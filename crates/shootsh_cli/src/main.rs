use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, MouseButton, MouseEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use rusqlite::Connection;
use shootsh_core::{
    Action, App,
    db::{DbRequest, Repository},
    domain, ui,
};
use std::{
    io,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

fn main() -> Result<()> {
    let conn = Connection::open("scores.db").context("Failed to open database")?;
    let repo = Repository::new(conn).context("Failed to initialize repository")?;

    let (db_tx, mut db_rx) = mpsc::channel::<DbRequest>(100);
    let shared_cache = Arc::new(Mutex::new(repo.get_top_scores(10).unwrap_or_default()));

    let worker_cache = Arc::clone(&shared_cache);
    std::thread::spawn(move || {
        while let Some(req) = db_rx.blocking_recv() {
            match req {
                DbRequest::SaveScore { name, score } => {
                    if let Ok(_) = repo.save_score(&name, score) {
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

    let mut app = App::new(db_tx, Arc::clone(&shared_cache));

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
    let res = run_loop(&mut app, &mut terminal);

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

    if app.high_score > 0 {
        println!("SESSION BEST: {}", app.high_score);
    }

    let final_name = domain::format_player_name(&app.player_name);
    println!("Good bye {}! See you next time at shoot.sh :)", final_name);

    Ok(())
}

fn run_loop<B: Backend>(app: &mut App, terminal: &mut Terminal<B>) -> Result<()>
where
    <B as Backend>::Error: std::error::Error + Send + Sync + 'static,
{
    let tick_rate = Duration::from_millis(16);
    let mut last_tick = Instant::now();

    while !app.should_quit {
        terminal.draw(|f| {
            let area = f.area();
            app.screen_size = domain::Size {
                width: area.width,
                height: area.height,
            };
            ui::render(app, f);
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
            handle_event(app, ev)?;
        }

        if last_tick.elapsed() >= tick_rate {
            app.update_state(Action::Tick)?;
            last_tick = Instant::now();
        }
    }
    Ok(())
}

fn handle_event(app: &mut App, event: Event) -> Result<()> {
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
        app.update_state(act)?;
    }
    Ok(())
}
