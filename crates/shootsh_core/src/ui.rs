use crate::app::{App, PlayingState, Scene};
use crate::db::DbCache;
use ratatui::{prelude::*, widgets::*};
use std::time::Duration;

const LOGO: &str = include_str!("./logo.txt");
pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;
const TABLE_WIDTH: u16 = 50;
const NAMING_INPUT_WIDTH: u16 = 40;

pub fn render(app: &App, cache: &DbCache, f: &mut Frame) {
    let area = f.area();

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        render_size_error(f, area);
        return;
    }

    match &app.scene {
        Scene::Naming(input_buffer) => render_naming(app, input_buffer, f, area),
        Scene::Menu => render_menu(app, cache, f, area),
        Scene::Playing(state) => render_playing(state, f, area),
        Scene::GameOver {
            final_score,
            is_new_record,
        } => render_game_over(app, cache, *final_score, *is_new_record, f, area),
    }
    render_warning(app, f, area);
    render_cursor(app, f);
}

fn render_warning(app: &App, f: &mut Frame, area: Rect) {
    if let Some(_) = app.last_cheat_warning {
        let warning_area = centered_rect(45, 5, area);

        f.render_widget(Clear, warning_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red).bold())
            .bg(Color::Black);

        let text = Paragraph::new(vec![
            Line::from("!! ABNORMAL BEHAVIOR DETECTED !!").red().bold(),
            Line::from("The interaction was discarded.").dark_gray(),
        ])
        .alignment(Alignment::Center)
        .block(block);

        f.render_widget(text, warning_area);
    }
}

fn render_cursor(app: &App, f: &mut Frame) {
    let area = f.area();

    let mut cursor_color = Color::LightGreen;

    if let Scene::Playing(state) = &app.scene {
        if state.target.is_hit(app.mouse_pos.x, app.mouse_pos.y) {
            cursor_color = Color::Yellow;
        }
    }

    let cursor_lines = vec!["  v  ", "- + -", "  ^  "];

    let cursor_height = cursor_lines.len() as u16;
    let cursor_width = cursor_lines.iter().map(|s| s.len()).max().unwrap_or(0) as u16;

    let offset_x = cursor_width / 2;
    let offset_y = cursor_height / 2;

    for (i, line) in cursor_lines.iter().enumerate() {
        for (j, ch) in line.chars().enumerate() {
            let x = app.mouse_pos.x as i32 + j as i32 - offset_x as i32;
            let y = app.mouse_pos.y as i32 + i as i32 - offset_y as i32;

            if x >= 0 && x < area.width as i32 && y >= 0 && y < area.height as i32 {
                if ch != ' ' {
                    f.render_widget(
                        Span::styled(ch.to_string(), Style::default().fg(cursor_color)),
                        Rect::new(x as u16, y as u16, 1, 1),
                    );
                }
            }
        }
    }
}

fn render_size_error(f: &mut Frame, area: Rect) {
    let msg = format!(
        "TERMINAL TOO SMALL\n\nRequired: {}x{}\nCurrent: {}x{}\n\nPlease resize!",
        MIN_WIDTH, MIN_HEIGHT, area.width, area.height
    );
    f.render_widget(
        Paragraph::new(msg)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Red).bold()),
        area,
    );
}

fn render_naming(_app: &App, input_buffer: &str, f: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .margin(5)
        .split(area);

    let input_area = centered_rect(NAMING_INPUT_WIDTH, 3, chunks[1]);

    f.render_widget(
        Paragraph::new("WELCOME TO SHOOT.SH")
            .alignment(Alignment::Center)
            .yellow()
            .bold(),
        chunks[0],
    );

    let input = Paragraph::new(input_buffer)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" ENTER YOUR NAME ")
                .title_alignment(Alignment::Center),
        )
        .alignment(Alignment::Center)
        .yellow();

    f.render_widget(input, input_area);

    f.render_widget(
        Paragraph::new("Press ENTER to start")
            .alignment(Alignment::Center)
            .dark_gray(),
        chunks[2],
    );
}

fn render_menu(app: &App, cache: &DbCache, f: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(10),
            Constraint::Length(4),
            Constraint::Min(0),
        ])
        .split(area);

    f.render_widget(
        Paragraph::new(LOGO)
            .alignment(Alignment::Center)
            .yellow()
            .bold(),
        chunks[0],
    );

    let mut lines = vec![Line::from("!!! CLICK TO START !!!").bold().slow_blink()];
    if app.user.high_score > 0 {
        lines.push(Line::from(format!("SESSION BEST: {}", app.user.high_score)).cyan());
    }
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        chunks[1],
    );
    render_leaderboard(app, cache, f, chunks[2], false);
}

fn render_playing(state: &PlayingState, f: &mut Frame, area: Rect) {
    let time_left = Duration::from_secs(crate::domain::PLAYING_TIME_SEC.into())
        .saturating_sub(state.scene_start.elapsed());

    let score = state.combat_stats.current_score();
    let combo = state.combat_stats.current_combo();

    let stats = Paragraph::new(format!(
        " SCORE: {} | COMBO {} | TIME: {}s ",
        score,
        combo,
        time_left.as_secs()
    ))
    .bold();

    f.render_widget(stats, Rect::new(area.x, area.y, area.width, 1));

    let target_rect = Rect::new(
        state.target.pos.x,
        state.target.pos.y,
        state.target.visual_width,
        state.target.visual_height,
    );

    let visible_rect = target_rect.intersection(area);

    if !visible_rect.is_empty() {
        f.render_widget(Block::default().bg(Color::Red), visible_rect);
    }
}

fn render_game_over(
    app: &App,
    cache: &DbCache,
    score: u32,
    is_new_record: bool,
    f: &mut Frame,
    area: Rect,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(4)
        .constraints([Constraint::Length(5), Constraint::Min(0)])
        .split(area);

    let msg = vec![
        Line::from(format!("FINAL SCORE: {}", score).bold().green()),
        Line::from(if is_new_record {
            "!!! NEW SESSION BEST !!!"
        } else {
            "TRY AGAIN!"
        })
        .yellow(),
        Line::from("Click to return Menu").italic(),
    ];
    f.render_widget(Paragraph::new(msg).alignment(Alignment::Center), chunks[0]);
    render_leaderboard(app, cache, f, chunks[1], true);
}

fn render_leaderboard(app: &App, cache: &DbCache, f: &mut Frame, area: Rect, _is_game_over: bool) {
    let rows: Vec<Row> = cache
        .all_time_scores
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let pos = i + 1;
            let is_own_entry = entry.name == app.user.name;
            let style = if is_own_entry {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            let pos_style = match pos {
                1 => Style::default().fg(Color::Yellow).bold(),
                2 => Style::default().fg(Color::Gray).bold(),
                3 => Style::default().fg(Color::Magenta).bold(),
                _ => Style::default().fg(Color::White),
            };

            Row::new(vec![
                Cell::from(format!("#{}", pos)).style(pos_style),
                Cell::from(entry.name.as_str()),
                Cell::from(entry.score.to_string()).fg(Color::Green),
                Cell::from(entry.created_at.as_str()),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Min(12),
            Constraint::Length(8),
            Constraint::Length(12),
        ],
    )
    .header(
        Row::new(vec!["RANK", "NAME", "SCORE", "DATE"])
            .underlined()
            .cyan(),
    )
    .block(
        Block::default()
            .title(" GLOBAL RANKING ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded),
    );

    f.render_widget(table, centered_rect(TABLE_WIDTH, area.height, area));
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y,
        width.min(area.width),
        height.min(area.height),
    )
}
