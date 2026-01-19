use crate::input::InputTransformer;
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect};
use russh::keys::ssh_key::PublicKey;
use russh::server::{Auth, Handler, Msg, Session};
use russh::*;
use shootsh_core::db::{DbCache, DbRequest};
use shootsh_core::{Action, App, domain, ui};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

const CLEANUP_SEQ: &[u8] = b"\x1b[?1003l\x1b[?1006l\x1b[?1049l\x1b[?25h";

/// A thread-safe wrapper around a byte buffer to capture TUI draw calls.
#[derive(Clone, Default)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

#[derive(Clone)]
pub struct MyServer {
    pub db_tx: mpsc::Sender<DbRequest>,
    pub shared_cache: Arc<Mutex<Arc<DbCache>>>,
    pub connection_count: Arc<AtomicUsize>,
}

impl russh::server::Server for MyServer {
    type Handler = ClientHandler;
    fn new_client(&mut self, peer_addr: Option<SocketAddr>) -> Self::Handler {
        let count = self.connection_count.fetch_add(1, Ordering::Relaxed) + 1;
        println!("New connection from {:?}. Active: {}", peer_addr, count);
        let (update_tx, update_rx) = mpsc::unbounded_channel();
        ClientHandler {
            db_tx: self.db_tx.clone(),
            shared_cache: self.shared_cache.clone(),
            app: None,
            input_transformer: InputTransformer::new(),
            terminal_size: Arc::new(Mutex::new(domain::Size {
                width: 80,
                height: 24,
            })),
            update_tx,
            update_rx: Arc::new(Mutex::new(Some(update_rx))),
            connection_count: self.connection_count.clone(),
            terminal: Arc::new(Mutex::new(None)),
            output_buffer: SharedBuffer::default(),
            fingerprint: Some(String::new()),
        }
    }
}

pub struct ClientHandler {
    db_tx: mpsc::Sender<DbRequest>,
    shared_cache: Arc<Mutex<Arc<DbCache>>>,
    app: Option<Arc<Mutex<App>>>,
    input_transformer: InputTransformer,
    terminal_size: Arc<Mutex<domain::Size>>,
    update_tx: mpsc::UnboundedSender<()>,
    update_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<()>>>>,
    connection_count: Arc<AtomicUsize>,
    terminal: Arc<Mutex<Option<Terminal<CrosstermBackend<SharedBuffer>>>>>,
    output_buffer: SharedBuffer,
    pub fingerprint: Option<String>,
}

impl ClientHandler {
    fn render_frame(
        app: &mut App,
        size: domain::Size,
        terminal_lock: &Arc<Mutex<Option<Terminal<CrosstermBackend<SharedBuffer>>>>>,
        shared_output: &SharedBuffer,
    ) -> Vec<u8> {
        let cache_snapshot: Arc<DbCache> = {
            let lock = app.db_cache.lock().unwrap();
            Arc::clone(&*lock)
        };

        app.screen_size = size;

        let mut terminal_guard = terminal_lock.lock().unwrap();
        if terminal_guard.is_none() {
            let backend = CrosstermBackend::new(shared_output.clone());
            let area = Rect::new(0, 0, size.width, size.height);
            *terminal_guard = Some(
                Terminal::with_options(
                    backend,
                    TerminalOptions {
                        viewport: Viewport::Fixed(area),
                    },
                )
                .expect("Failed to create terminal"),
            );
        }

        let terminal = terminal_guard.as_mut().unwrap();

        let current_area = Rect::new(0, 0, size.width, size.height);
        if terminal.size().unwrap() != current_area.into() {
            terminal
                .resize(current_area)
                .expect("Failed to resize terminal");
        }

        // Write Arc<Mutex<Vec<u8>>> via SharedBuffer
        terminal
            .draw(|f| {
                ui::render(app, &cache_snapshot, f);
                f.set_cursor_position(ratatui::layout::Position::new(0, 0));
            })
            .expect("Failed to draw frame");

        let mut output = Vec::new();
        output.extend_from_slice(b"\x1b[?25l");

        let mut internal_vec = shared_output.0.lock().unwrap();
        output.extend(std::mem::take(&mut *internal_vec));

        output
    }
}
impl Handler for ClientHandler {
    type Error = russh::Error;

    // async fn auth_none(&mut self, _user: &str) -> std::result::Result<Auth, Self::Error> {
    //     Ok(Auth::Accept)
    // }

    async fn auth_publickey(&mut self, _user: &str, key: &PublicKey) -> Result<Auth, Self::Error> {
        let fp = key
            .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
            .to_string();
        self.fingerprint = Some(fp);

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
        let fp = self.fingerprint.clone().expect("Should be authenticated");
        let (tx, rx) = std::sync::mpsc::channel();

        // send request
        self.db_tx
            .send(DbRequest::GetOrCreateUser {
                fingerprint: fp.clone(),
                reply_tx: tx,
            })
            .await
            .map_err(|_| russh::Error::Inconsistent)?;

        let user_context =
            tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(2)))
                .await
                .map_err(|_| russh::Error::Inconsistent)? // JoinError
                .map_err(|_| russh::Error::Inconsistent)?; // RecvTimeoutError

        let initial_size = *self.terminal_size.lock().unwrap();
        let mut app = App::new(user_context, self.db_tx.clone(), self.shared_cache.clone());
        app.screen_size = initial_size;

        let app_arc = Arc::new(Mutex::new(app));
        self.app = Some(app_arc.clone());

        let terminal_handle = self.terminal.clone();
        let output_handle = self.output_buffer.clone();

        let _ = session.channel_success(channel);
        let _ = session.data(channel, "\x1b[?1049h\x1b[?1003h\x1b[?1006h\x1b[?25l".into());

        let mut rx = self.update_rx.lock().unwrap().take();
        let session_handle = session.handle();
        let size_handle = self.terminal_size.clone();

        tokio::spawn(async move {
            struct DropGuard {
                handle: russh::server::Handle,
                chan: ChannelId,
            }
            impl Drop for DropGuard {
                fn drop(&mut self) {
                    let h = self.handle.clone();
                    let c = self.chan;
                    tokio::spawn(async move {
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

                let render_result = {
                    let mut app = match app_arc.lock() {
                        Ok(a) => a,
                        Err(_) => break,
                    };
                    let sz = *size_handle.lock().unwrap();
                    app.update_state(Action::Tick).ok();
                    (
                        Self::render_frame(&mut app, sz, &terminal_handle, &output_handle),
                        app.should_quit,
                    )
                };

                let (buffer, should_quit) = render_result;
                if session_handle.data(channel, buffer.into()).await.is_err() || should_quit {
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

        let actions = self.input_transformer.handle_input(data);

        if !actions.is_empty() {
            let buffer = {
                let mut app = app_arc.lock().unwrap();
                let sz = *self.terminal_size.lock().unwrap();
                for act in actions {
                    app.update_state(act).ok();
                }
                Self::render_frame(&mut app, sz, &self.terminal, &self.output_buffer)
            };
            let _ = session.data(channel, buffer.into());
        }

        Ok(())
    }
}

impl Drop for ClientHandler {
    fn drop(&mut self) {
        let count = self.connection_count.fetch_sub(1, Ordering::Relaxed) - 1;
        println!("Connection closed. Active: {}", count);
    }
}
