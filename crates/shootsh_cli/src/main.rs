use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use crossterm::{
    event::{self, Event, KeyCode, MouseButton, MouseEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use rusqlite::Connection;
use shootsh_core::Scene;
use shootsh_core::db::DbCache;
use shootsh_core::{
    Action, App,
    db::{DbRequest, Repository},
    domain, ui,
};
use std::{
    io,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

const DEFAULT_MAX_USERS: i64 = 100_000;

#[tokio::main]
async fn main() -> Result<()> {
    let conn = Connection::open("shootsh.db").context("Failed to open database")?;
    let repo =
        Repository::new(conn, DEFAULT_MAX_USERS).context("Failed to initialize repository")?;
    let shared_cache = Arc::new(ArcSwap::from_pointee(repo.get_current_cache()));
    let (db_tx, db_rx) = mpsc::channel::<DbRequest>(100);

    let user_context = repo
        .get_or_create_user_context("local")
        .context("Failed to get or create local user")?;
    let mut app = App::new(user_context, db_tx, shared_cache.load_full());

    spawn_db_worker(repo, Arc::clone(&shared_cache), db_rx);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture)?;

    let panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            event::DisableMouseCapture
        );
        panic_hook(panic_info);
    }));

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    let res = run_loop(&mut app, &mut terminal, shared_cache).await;

    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        event::DisableMouseCapture
    )
    .ok();
    disable_raw_mode().ok();

    if let Err(e) = res {
        eprintln!("Application Error: {:?}", e);
    }

    Ok(())
}

async fn run_loop<B: Backend>(
    app: &mut App,
    terminal: &mut Terminal<B>,
    shared_cache: Arc<ArcSwap<shootsh_core::db::DbCache>>,
) -> Result<()>
where
    <B as Backend>::Error: std::error::Error + Send + Sync + 'static,
{
    let tick_rate = Duration::from_millis(16);
    let mut last_tick = Instant::now();

    while !app.should_quit {
        app.db_cache = shared_cache.load_full();

        if let Ok(size) = terminal.size() {
            app.screen_size = domain::Size {
                width: size.width,
                height: size.height,
            };
        }

        terminal.draw(|f| {
            ui::render(app, &app.db_cache, f);
        })?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            let ev = event::read()?;
            if let Event::Resize(w, h) = ev {
                app.screen_size = domain::Size {
                    width: w,
                    height: h,
                };
            }
            handle_event(app, ev).await?;
        }

        if last_tick.elapsed() >= tick_rate {
            app.update_state(Action::Tick).0?;
            last_tick = Instant::now();
        }
    }
    Ok(())
}

async fn handle_event(app: &mut App, event: Event) -> Result<()> {
    let action = match event {
        Event::Key(key) => {
            if key.modifiers.contains(event::KeyModifiers::CONTROL)
                && key.code == KeyCode::Char('c')
            {
                Some(Action::Quit)
            } else {
                match key.code {
                    KeyCode::Enter => Some(Action::SubmitName),
                    KeyCode::Char(c) => Some(Action::InputChar(c)),
                    KeyCode::Backspace => Some(Action::DeleteChar),
                    KeyCode::Esc => Some(Action::BackToMenu),
                    _ => None,
                }
            }
        }
        Event::Mouse(m) => match m.kind {
            MouseEventKind::Down(MouseButton::Left) => Some(Action::MouseClick(m.column, m.row)),
            MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                Some(Action::MouseMove(m.column, m.row))
            }
            _ => None,
        },
        _ => None,
    };

    if let Some(act) = action {
        let (res, rx) = app.update_state(act);
        res.context("Failed to update state")?;

        if let Some(rx) = rx {
            // for a CLI ver, this is not a matter
            match rx.await {
                Ok(Ok(_)) => {
                    if let Scene::Naming(state) = &app.scene {
                        app.user.name = Some(state.input.clone());
                    }
                    app.change_scene(Scene::Menu);
                }
                Ok(Err(e)) => {
                    if let Scene::Naming(state) = &mut app.scene {
                        state.error = Some(e);
                        state.is_loading = false;
                    }
                }
                Err(_) => {
                    if let Scene::Naming(state) = &mut app.scene {
                        state.error = Some("Internal communication error".into());
                        state.is_loading = false;
                    }
                }
            }
        }
    }
    Ok(())
}

fn spawn_db_worker(
    repo: Repository,
    cache: Arc<ArcSwap<DbCache>>,
    mut rx: mpsc::Receiver<DbRequest>,
) {
    std::thread::spawn(move || {
        while let Some(req) = rx.blocking_recv() {
            match repo.handle_request(req) {
                Some(new_cache) => {
                    cache.store(Arc::new(new_cache));
                }
                None => {}
            }
        }
    });
}
