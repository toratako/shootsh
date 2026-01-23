use crate::input::InputTransformer;
use arc_swap::ArcSwap;
use crossterm::style::{Color, Stylize};
use futures::future::join_all;
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect};
use russh::keys::ssh_key::PublicKey;
use russh::server::{Auth, Handler, Msg, Session};
use russh::*;
use shootsh_core::Scene;
use shootsh_core::db::{DbCache, DbRequest};
use shootsh_core::{Action, App, domain, ui};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::Instrument;

const SETUP_SEQ: &[u8] = concat!(
    "\x1b[?1049h", // EnterAlternateScreen
    "\x1b[?1000h", // EnableMouseCapture (Normal)
    "\x1b[?1002h", // EnableMouseCapture (Button)
    "\x1b[?1003h", // EnableMouseCapture (Any)
    "\x1b[?1015h", // EnableMouseCapture (URXVT)
    "\x1b[?1006h", // EnableMouseCapture (SGR)
    "\x1b[?25l"    // HideCursor
)
.as_bytes();

const CLEANUP_SEQ: &[u8] = concat!(
    "\x1b[?1006l", // DisableMouseCapture (SGR)
    "\x1b[?1015l", // DisableMouseCapture (URXVT)
    "\x1b[?1003l", // DisableMouseCapture (Any)
    "\x1b[?1002l", // DisableMouseCapture (Button)
    "\x1b[?1000l", // DisableMouseCapture (Normal)
    "\x1b[?1049l", // LeaveAlternateScreen
    "\x1b[?25h"    // ShowCursor
)
.as_bytes();

const CURSOR_HIDE: &[u8] = b"\x1b[?25l";

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

pub struct SessionInfo {
    pub handle: russh::server::Handle,
    pub channel_id: ChannelId,
}

#[derive(Clone)]
pub struct MyServer {
    pub db_tx: mpsc::Sender<DbRequest>,
    pub shared_cache: Arc<ArcSwap<DbCache>>,
    pub connection_count: Arc<AtomicUsize>,
    pub active_sessions: Arc<Mutex<HashMap<String, SessionInfo>>>,
}

impl MyServer {
    pub async fn cleanup_all_sessions(&self) {
        let mut sessions = self.active_sessions.lock().unwrap();
        let session_list: Vec<_> = sessions.drain().collect();
        drop(sessions);

        let shutdown_msg = format!("\r\n{}\r\n", "Server is shutting down.".red().bold());

        let tasks = session_list.into_iter().map(|(fp, info)| {
            let msg = shutdown_msg.clone();
            async move {
                let mut payload = Vec::from(CLEANUP_SEQ);
                payload.extend_from_slice(msg.as_bytes());
                let _ = info.handle.data(info.channel_id, payload.into()).await;
                let _ = info.handle.eof(info.channel_id).await;

                let _ = info.handle.close(info.channel_id).await;

                tracing::info!(fingerprint = %fp, "Cleanup task finished");
            }
        });

        join_all(tasks).await;
    }
}

impl russh::server::Server for MyServer {
    type Handler = ClientHandler;
    fn new_client(&mut self, peer_addr: Option<SocketAddr>) -> Self::Handler {
        let count = self.connection_count.fetch_add(1, Ordering::Relaxed) + 1;

        let span = tracing::info_span!("client", addr = ?peer_addr, fp = tracing::field::Empty);
        let _enter = span.enter();
        tracing::info!(active_connections = count, "New connection");

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
            update_rx: Some(update_rx),
            connection_count: self.connection_count.clone(),
            terminal: None,
            output_buffer: SharedBuffer::default(),
            fingerprint: None,
            active_sessions: self.active_sessions.clone(),
            span: span.clone(),
        }
    }
}

pub struct ClientHandler {
    db_tx: mpsc::Sender<DbRequest>,
    pub shared_cache: Arc<ArcSwap<DbCache>>,
    app: Option<Arc<Mutex<App>>>,
    input_transformer: InputTransformer,
    terminal_size: Arc<Mutex<domain::Size>>,
    update_tx: mpsc::UnboundedSender<()>,
    update_rx: Option<mpsc::UnboundedReceiver<()>>,
    connection_count: Arc<AtomicUsize>,
    terminal: Option<Terminal<CrosstermBackend<SharedBuffer>>>,
    output_buffer: SharedBuffer,
    pub fingerprint: Option<String>,
    pub active_sessions: Arc<Mutex<HashMap<String, SessionInfo>>>,
    pub span: tracing::Span,
}

impl ClientHandler {
    fn render_frame(
        app: &App,
        terminal: &mut Terminal<CrosstermBackend<SharedBuffer>>,
        shared_output: &SharedBuffer,
    ) -> Vec<u8> {
        terminal
            .draw(|f| {
                ui::render(app, &app.db_cache, f);
                f.set_cursor_position(ratatui::layout::Position::new(0, 0));
            })
            .expect("Failed to draw frame");

        let mut output = Vec::from(CURSOR_HIDE);
        let mut internal_vec = shared_output.0.lock().unwrap();
        output.extend(std::mem::take(&mut *internal_vec));

        output
    }

    async fn kick_existing_session(
        &self,
        fp: &str,
        channel: ChannelId,
        current_handle: russh::server::Handle,
    ) {
        let old_session = {
            let mut sessions = self.active_sessions.lock().unwrap();
            sessions.insert(
                fp.to_string(),
                SessionInfo {
                    handle: current_handle,
                    channel_id: channel,
                },
            )
        };

        if let Some(old_session) = old_session {
            let _ = old_session
                .handle
                .data(old_session.channel_id, CLEANUP_SEQ.into())
                .await;
            let _ = old_session.handle.close(old_session.channel_id).await;
        }
    }

    async fn fetch_user_context(
        &self,
        fp: &str,
    ) -> Result<shootsh_core::db::UserContext, russh::Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();

        self.db_tx
            .send(DbRequest::GetOrCreateUser {
                fingerprint: fp.to_string(),
                reply_tx: tx,
            })
            .await
            .map_err(|_| russh::Error::Inconsistent)?;

        tokio::time::timeout(Duration::from_secs(2), rx)
            .await
            .map_err(|_| {
                tracing::error!(reason = "timeout", "Login failed");
                russh::Error::Inconsistent
            })? // timeout error
            .map_err(|_| {
                tracing::error!(reason = "error", "Login failed");
                russh::Error::Inconsistent
            }) // oneshot recv error
    }

    fn run_render_loop(
        &mut self,
        channel: ChannelId,
        session_handle: russh::server::Handle,
        app: Arc<Mutex<App>>,
    ) {
        let span = self.span.clone();

        let mut rx = self
            .update_rx
            .take()
            .expect("Internal error: update_rx already taken");
        let mut term = self.terminal.take();
        let terminal_size = self.terminal_size.clone();
        let shared_cache = self.shared_cache.clone();
        let output_buffer = self.output_buffer.clone();

        tokio::spawn(
            async move {
                tracing::debug!("Render loop started");

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
                        res = rx.recv() => {
                            if res.is_none() { break; }
                        },
                    }

                    let render_result = {
                        let mut app = app.lock().unwrap();
                        let sz = *terminal_size.lock().unwrap();
                        app.db_cache = shared_cache.load_full();

                        app.update_state(Action::Tick).0.ok();

                        let t = term.get_or_insert_with(|| {
                            let backend = CrosstermBackend::new(output_buffer.clone());
                            Terminal::with_options(
                                backend,
                                TerminalOptions {
                                    viewport: Viewport::Fixed(Rect::new(0, 0, sz.width, sz.height)),
                                },
                            )
                            .expect("Failed to create terminal")
                        });

                        let current_area = Rect::new(0, 0, sz.width, sz.height);
                        if t.size().unwrap() != current_area.into() {
                            t.resize(current_area).ok();
                        }

                        (Self::render_frame(&app, t, &output_buffer), app.should_quit)
                    };

                    let (buffer, should_quit) = render_result;
                    if session_handle.data(channel, buffer.into()).await.is_err() || should_quit {
                        break;
                    }
                }
                tracing::debug!("Render loop finished");
            }
            .instrument(span),
        );
    }
}

impl Handler for ClientHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, _user: &str, _pass: &str) -> Result<Auth, Self::Error> {
        Ok(Auth::Accept)
    }

    async fn auth_publickey(&mut self, _user: &str, key: &PublicKey) -> Result<Auth, Self::Error> {
        let fp = key
            .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
            .to_string();

        self.span.record("fp", &fp);
        self.span.in_scope(|| {
            tracing::info!("Public key authenticated");
        });

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
        let fp = match self.fingerprint.clone() {
            Some(fp) => fp,
            None => {
                self.span.in_scope(|| {
                    tracing::warn!("Password authentication rejected (Public key required)");
                });

                let error_header = "Error: Public key authentication is required."
                    .with(Color::Red)
                    .bold();
                let command_hint = "ssh-keygen -t ed25519".with(Color::Cyan);

                let msg = format!(
                    "\r\n{}\r\n\
                    Please generate a key using: {}\r\n\r\n",
                    error_header, command_hint
                );

                let _ = session.data(channel, msg.into());
                let _ = session.channel_success(channel);
                let _ = session.close(channel);
                return Ok(());
            }
        };

        self.kick_existing_session(&fp, channel, session.handle())
            .await;

        let user_context = self.fetch_user_context(&fp).await?;

        self.active_sessions.lock().unwrap().insert(
            fp.clone(),
            SessionInfo {
                handle: session.handle(),
                channel_id: channel,
            },
        );

        let initial_cache = self.shared_cache.load_full();
        let mut app = App::new(user_context, self.db_tx.clone(), initial_cache);
        let initial_size = *self.terminal_size.lock().unwrap();
        app.screen_size = initial_size;

        let app_arc = Arc::new(Mutex::new(app));
        self.app = Some(app_arc.clone());

        let _ = session.channel_success(channel);
        let _ = session.data(channel, SETUP_SEQ.into());

        self.run_render_loop(channel, session.handle(), app_arc);

        Ok(())
    }

    async fn data(
        &mut self,
        _channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> std::result::Result<(), Self::Error> {
        let app_arc = match &self.app {
            Some(a) => a,
            None => return Ok(()),
        };

        let actions = self.input_transformer.handle_input(data);

        if !actions.is_empty() {
            let mut pending_workers = Vec::new();

            {
                let mut app = app_arc.lock().unwrap();
                app.screen_size = *self.terminal_size.lock().unwrap();

                for act in actions {
                    let (res, rx) = app.update_state(act);

                    if res.is_err() {
                        continue;
                    }

                    if let Some(r) = rx {
                        pending_workers.push(r);
                    }
                }
            }
            for rx in pending_workers {
                let app_clone = app_arc.clone();
                let update_tx = self.update_tx.clone();

                tokio::spawn(async move {
                    if let Ok(result) = rx.await {
                        let mut app_inner = app_clone.lock().unwrap();
                        match result {
                            Ok(_) => {
                                if let Scene::Naming(state) = &app_inner.scene {
                                    app_inner.user.name = Some(state.input.clone());
                                }
                                app_inner.change_scene(Scene::Menu);
                            }
                            Err(e) => {
                                if let Scene::Naming(state) = &mut app_inner.scene {
                                    state.error = Some(e);
                                    state.is_loading = false;
                                }
                            }
                        }
                        let _ = update_tx.send(());
                    }
                });
            }

            let _ = self.update_tx.send(());
        }

        Ok(())
    }
}

impl Drop for ClientHandler {
    fn drop(&mut self) {
        // DO NOT REMOVE SESSION HERE! (kick_existing_session handles this well)
        let _enter = self.span.enter();
        let count = self.connection_count.fetch_sub(1, Ordering::Relaxed) - 1;
        tracing::info!(active_connections = count, "Connection closed");
    }
}
