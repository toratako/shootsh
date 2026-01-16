use crate::app::{App, PLAYING_TIME, Scene};
use ratatui::{prelude::*, widgets::*};
use std::time::Duration;

const LOGO: &str = include_str!("./logo.txt");
pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;
const TABLE_WIDTH: u16 = 50;
const NAMING_INPUT_WIDTH: u16 = 40;

pub fn render(app: &App, f: &mut Frame) {
    let area = f.area();

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        render_size_error(f, area);
        return;
    }

    match &app.scene {
        Scene::Naming => render_naming(app, f, area),
        Scene::Menu => render_menu(app, f, area),
        Scene::Playing { target } => render_playing(app, target, f, area),
        Scene::GameOver {
            final_score,
            is_new_record,
        } => render_game_over(app, *final_score, *is_new_record, f, area),
    }
    render_cursor(app, f);
}

fn render_cursor(app: &App, f: &mut Frame) {
    let area = f.area();
    if app.mouse_pos.0 < area.width && app.mouse_pos.1 < area.height {
        f.render_widget(
            Span::styled("+", Style::default().fg(Color::Cyan)),
            Rect::new(app.mouse_pos.0, app.mouse_pos.1, 1, 1),
        );
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

fn render_naming(app: &App, f: &mut Frame, area: Rect) {
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

    let input = Paragraph::new(app.player_name.as_str())
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

fn render_menu(app: &App, f: &mut Frame, area: Rect) {
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
    if app.high_score > 0 {
        lines.push(Line::from(format!("SESSION BEST: {}", app.high_score)).cyan());
    }
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        chunks[1],
    );
    render_leaderboard(app, f, chunks[2], false);
}

fn render_playing(app: &App, target: &crate::domain::Target, f: &mut Frame, area: Rect) {
    let time_left =
        Duration::from_secs(PLAYING_TIME.into()).saturating_sub(app.last_scene_change.elapsed());

    let stats = Paragraph::new(format!(
        " SCORE: {} | TIME: {}s ",
        app.current_score,
        time_left.as_secs()
    ))
    .bold();

    f.render_widget(stats, Rect::new(area.x, area.y, area.width, 1));

    let target_rect = Rect::new(target.pos.x, target.pos.y, target.visual_width, 1);
    let visible_rect = target_rect.intersection(area);

    if !visible_rect.is_empty() {
        f.render_widget(Block::default().bg(Color::Red), visible_rect);
    }
}
fn render_game_over(app: &App, score: u32, is_new_record: bool, f: &mut Frame, area: Rect) {
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
    render_leaderboard(app, f, chunks[1], true);
}

fn render_leaderboard(app: &App, f: &mut Frame, area: Rect, is_game_over: bool) {
    let rows: Vec<Row> = app
        .ranking_cache
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let pos = i + 1;
            let is_own_entry = is_game_over && entry.name == app.player_name;
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
