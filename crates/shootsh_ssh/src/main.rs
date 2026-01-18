use anyhow::{Context, Result};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect};
use rusqlite::Connection;
use russh::keys::ssh_key::rand_core::OsRng;
use russh::server::{Auth, Handler, Msg, Server as _, Session};
use russh::*;
use shootsh_core::db::{DbRequest, Repository, ScoreEntry};
use shootsh_core::{Action, App, domain, ui};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use termwiz::input::{InputEvent, InputParser, KeyCode, Modifiers, MouseButtons};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

const CLEANUP_SEQ: &[u8] = b"\x1b[?1003l\x1b[?1006l\x1b[?1049l\x1b[?25h";

#[tokio::main]
async fn main() -> Result<()> {
    let conn = Connection::open("scores.db").context("Failed to open DB")?;
    let repo = Repository::new(conn).context("Failed to init repo")?;
    let (db_tx, mut db_rx) = mpsc::channel::<DbRequest>(100);

    let initial_scores = repo.get_top_scores(10).unwrap_or_default();
    let shared_cache = Arc::new(Mutex::new(initial_scores));
    let worker_cache = Arc::clone(&shared_cache);

    std::thread::spawn(move || {
        while let Some(req) = db_rx.blocking_recv() {
            match req {
                DbRequest::SaveScore { name, score } => {
                    if repo.save_score(&name, score).is_ok() {
                        if let Ok(new_ranks) = repo.get_top_scores(10) {
                            if let Ok(mut lock) = worker_cache.lock() {
                                *lock = new_ranks;
                            }
                        }
                    }
                }
            }
        }
    });

    let config = russh::server::Config {
        inactivity_timeout: Some(Duration::from_secs(3600)),
        auth_rejection_time: Duration::from_secs(3),
        keys: vec![
            russh::keys::PrivateKey::random(&mut OsRng, russh::keys::Algorithm::Ed25519)
                .map_err(|e| anyhow::anyhow!("Key gen failed: {}", e))?,
        ],
        ..Default::default()
    };
    let config = Arc::new(config);

    let mut sh = MyServer {
        db_tx,
        shared_cache,
    };

    let addr = "0.0.0.0:2222";
    let socket = TcpListener::bind(addr).await?;
    println!("Starting shootsh_ssh on {}", addr);

    sh.run_on_socket(config, &socket).await?;
    Ok(())
}

#[derive(Clone)]
struct MyServer {
    db_tx: mpsc::Sender<DbRequest>,
    shared_cache: Arc<Mutex<Vec<ScoreEntry>>>,
}

impl russh::server::Server for MyServer {
    type Handler = ClientHandler;
    fn new_client(&mut self, _peer_addr: Option<SocketAddr>) -> Self::Handler {
        let (update_tx, update_rx) = mpsc::unbounded_channel();
        ClientHandler {
            db_tx: self.db_tx.clone(),
            shared_cache: self.shared_cache.clone(),
            app: None,
            parser: InputParser::new(),
            terminal_size: Arc::new(Mutex::new(domain::Size {
                width: 80,
                height: 24,
            })),
            update_tx,
            update_rx: Arc::new(Mutex::new(Some(update_rx))),
        }
    }
}

pub struct ClientHandler {
    db_tx: mpsc::Sender<DbRequest>,
    shared_cache: Arc<Mutex<Vec<ScoreEntry>>>,
    app: Option<Arc<Mutex<App>>>,
    parser: InputParser,
    terminal_size: Arc<Mutex<domain::Size>>,
    update_tx: mpsc::UnboundedSender<()>,
    update_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<()>>>>,
}

impl ClientHandler {
    fn render_frame(app: &mut App, size: domain::Size) -> Vec<u8> {
        let mut buffer = Vec::new();
        app.screen_size = size;
        {
            let backend = CrosstermBackend::new(&mut buffer);
            let area = Rect::new(0, 0, size.width, size.height);
            let mut terminal = Terminal::with_options(
                backend,
                TerminalOptions {
                    viewport: Viewport::Fixed(area),
                },
            )
            .unwrap();

            // fix corruption of diff rendering of ratatui
            terminal.clear().unwrap();
            terminal
                .draw(|f| {
                    ui::render(app, f);
                    f.set_cursor_position(ratatui::layout::Position::new(0, 0));
                })
                .unwrap();
        }
        buffer.extend_from_slice(b"\x1b[?25l");
        buffer
    }
}

impl Handler for ClientHandler {
    type Error = russh::Error;

    async fn auth_none(&mut self, _user: &str) -> std::result::Result<Auth, Self::Error> {
        Ok(Auth::Accept)
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        _session: &mut Session,
    ) -> std::result::Result<bool, Self::Error> {
        Ok(true)
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(Pty, u32)],
        session: &mut Session,
    ) -> std::result::Result<(), Self::Error> {
        if let Ok(mut sz) = self.terminal_size.lock() {
            *sz = domain::Size {
                width: col_width as u16,
                height: row_height as u16,
            };
        }
        let _ = session.channel_success(channel);
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _session: &mut Session,
    ) -> std::result::Result<(), Self::Error> {
        if let Ok(mut sz) = self.terminal_size.lock() {
            *sz = domain::Size {
                width: col_width as u16,
                height: row_height as u16,
            };
        }
        let _ = self.update_tx.send(());
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> std::result::Result<(), Self::Error> {
        let initial_size = *self.terminal_size.lock().unwrap();
        let mut app = App::new(self.db_tx.clone(), self.shared_cache.clone());
        app.screen_size = initial_size;

        let app_arc = Arc::new(Mutex::new(app));
        self.app = Some(app_arc.clone());

        let _ = session.channel_success(channel);

        // capture mouse movement
        let _ = session.data(channel, "\x1b[?1049h\x1b[?1003h\x1b[?1006h\x1b[?25l".into());

        let mut rx = self.update_rx.lock().unwrap().take();
        let session_handle = session.handle();
        let size_handle = self.terminal_size.clone();

        tokio::spawn(async move {
            // cleanup guard
            struct DropGuard {
                handle: russh::server::Handle,
                chan: ChannelId,
            }
            impl Drop for DropGuard {
                fn drop(&mut self) {
                    let h = self.handle.clone();
                    let c = self.chan;
                    tokio::spawn(async move {
                        // cleanup. disable mouse, show mouse...
                        let _ = h.data(c, CLEANUP_SEQ.into()).await;
                        let _ = h.close(c).await;
                    });
                }
            }
            let _guard = DropGuard {
                handle: session_handle.clone(),
                chan: channel,
            };

            let mut interval = tokio::time::interval(Duration::from_millis(33));
            loop {
                tokio::select! {
                    _ = interval.tick() => {},
                    res = async {
                        if let Some(ref mut r) = rx { r.recv().await } else { None }
                    } => {
                        if res.is_none() && rx.is_some() { break; }
                    },
                }

                let (buffer, should_quit) = {
                    let mut app = match app_arc.lock() {
                        Ok(a) => a,
                        Err(_) => break,
                    };
                    let sz = *size_handle.lock().unwrap();
                    app.update_state(Action::Tick).ok();
                    (Self::render_frame(&mut app, sz), app.should_quit)
                };

                if session_handle.data(channel, buffer.into()).await.is_err() || should_quit {
                    // if drop _guard, CLEANUP_SEQ will be sent
                    break;
                }
            }
        });

        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> std::result::Result<(), Self::Error> {
        let app_arc = match &self.app {
            Some(a) => a,
            None => return Ok(()),
        };

        let mut actions = Vec::new();
        self.parser.parse(
            data,
            |event| {
                match event {
                    InputEvent::Key(k) => {
                        if k.modifiers.contains(Modifiers::CTRL) && k.key == KeyCode::Char('c') {
                            actions.push(Action::Quit);
                        } else {
                            match k.key {
                                KeyCode::Enter => actions.push(Action::SubmitName),
                                KeyCode::Backspace => actions.push(Action::DeleteChar),
                                KeyCode::Escape => actions.push(Action::BackToMenu),
                                KeyCode::Char(c) => actions.push(Action::InputChar(c)),
                                _ => {}
                            }
                        }
                    }
                    InputEvent::Mouse(m) => {
                        // conver termwiz coord(1-based) to 0-indexed
                        let x = m.x.saturating_sub(1);
                        let y = m.y.saturating_sub(1);

                        // follow all mouse movement
                        if m.mouse_buttons.contains(MouseButtons::LEFT) {
                            actions.push(Action::MouseClick(x, y));
                        } else {
                            actions.push(Action::MouseMove(x, y));
                        }
                    }
                    _ => {}
                }
            },
            false,
        );

        if !actions.is_empty() {
            let buffer = {
                let mut app = app_arc.lock().unwrap();
                let sz = *self.terminal_size.lock().unwrap();
                for act in actions {
                    app.update_state(act).ok();
                }
                Self::render_frame(&mut app, sz)
            };
            // send mouse move
            let _ = session.data(channel, buffer.into());
        }

        Ok(())
    }
}
