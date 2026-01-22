use crate::app::{App, NamingState, PlayingState, Scene};
use crate::db::DbCache;
use chrono::{Datelike, Utc};
use ratatui::{prelude::*, widgets::*};
use std::time::Duration;

const LOGO: &str = include_str!("./logo.txt");
pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;
const TABLE_WIDTH: u16 = 50;
const NAMING_INPUT_WIDTH: u16 = 40;

const DAYS_IN_WEEK: u16 = 7;
const WEEKS_TO_DISPLAY: u16 = 15;

pub fn render(app: &App, cache: &DbCache, f: &mut Frame) {
    let area = f.area();

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        render_size_error(f, area);
        return;
    }

    match &app.scene {
        Scene::Naming(state) => render_naming(app, state, f, area),
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
        let warning_area = absolute_centered_rect(45, 5, area);

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

    let mut style = Style::default().fg(Color::LightGreen);

    if let Scene::Playing(state) = &app.scene {
        if state.target.is_hit(app.mouse_pos.x, app.mouse_pos.y) {
            style = style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
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
                        Span::styled(ch.to_string(), style),
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

fn render_naming(_app: &App, state: &NamingState, f: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .margin(5)
        .split(area);

    let input_area = absolute_centered_rect(NAMING_INPUT_WIDTH, 3, chunks[1]);

    f.render_widget(
        Paragraph::new("WELCOME TO SHOOT.SH")
            .alignment(Alignment::Center)
            .yellow()
            .bold(),
        chunks[0],
    );

    let input_text = if state.is_loading {
        format!("{} (Saving...)", state.input)
    } else {
        state.input.clone()
    };

    let input = Paragraph::new(input_text.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" ENTER YOUR NAME ")
                .title_alignment(Alignment::Center),
        )
        .alignment(Alignment::Center)
        .style(if state.is_loading {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Yellow)
        });

    f.render_widget(input, input_area);

    if let Some(ref err) = state.error {
        f.render_widget(
            Paragraph::new(err.as_str())
                .style(Style::default().fg(Color::Red))
                .alignment(Alignment::Center),
            chunks[2],
        );
    }

    let footer_text = if state.is_loading {
        "Please wait..."
    } else {
        "Press ENTER to start"
    };

    f.render_widget(
        Paragraph::new(footer_text)
            .alignment(Alignment::Center)
            .dark_gray(),
        chunks[3],
    );
}

fn render_menu(app: &App, cache: &DbCache, f: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(7), // logo
            Constraint::Length(9), // activity
            Constraint::Length(4), // message
            Constraint::Min(0),    // leaderboard
        ])
        .split(area);

    // logo
    let logo_width = LOGO.lines().map(|l| l.len()).max().unwrap_or(0) as u16;
    let logo_height = LOGO.lines().count() as u16;
    let logo_area = horizontal_centered_rect(logo_width, logo_height, chunks[0]);
    f.render_widget(Paragraph::new(LOGO).yellow().bold(), logo_area);

    // activity
    render_activity_graph(app, f, chunks[1]);

    let mut lines = vec![Line::from("!!! CLICK TO START !!!").bold().slow_blink()];
    if app.user.high_score > 0 {
        lines.push(Line::from(format!("HIGH SCORE: {}", app.user.high_score)).cyan());
    }
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        chunks[2],
    );

    // leaderboard
    render_leaderboard(app, cache, f, chunks[3], false);
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
            "!!! NEW HIGH SCORE !!!"
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
            let is_own_entry = app.user.name.as_ref() == Some(&entry.name);
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
            .borders(Borders::ALL),
    );

    // 3 = header, borders...
    let table_height = (cache.all_time_scores.len() as u16 + 3).min(area.height);
    f.render_widget(
        table,
        horizontal_centered_rect(TABLE_WIDTH, table_height, area),
    );
}

fn render_activity_graph(app: &App, f: &mut Frame, area: Rect) {
    let title = format!(" ACTIVITY ({}weeks) ", WEEKS_TO_DISPLAY);
    let label_width = 2; // "S ", "M ", ...
    let today = Utc::now().date_naive();
    let days_from_sunday = today.weekday().num_days_from_sunday() as i64;
    let total_days_to_show = WEEKS_TO_DISPLAY as i64 * 7;
    let start_date = today - chrono::Duration::days(days_from_sunday + (total_days_to_show - 7));

    let labels = ["S", "M", "T", "W", "T", "F", "S"];
    let mut lines = Vec::new();

    for day_offset in 0..DAYS_IN_WEEK {
        let mut line_spans = Vec::new();

        // S, M, T...
        line_spans.push(Span::styled(
            format!("{} ", labels[day_offset as usize]),
            Style::default().dark_gray(),
        ));

        for week in 0..WEEKS_TO_DISPLAY {
            let current_date =
                start_date + chrono::Duration::days((week as i64 * 7) + day_offset as i64);

            let date_str = current_date.format("%Y-%m-%d").to_string();
            let activity_count = app
                .user
                .user_activity
                .iter()
                .find(|a| a.date == date_str)
                .map(|a| a.count)
                .unwrap_or(0);

            let display_text = if current_date > today {
                "  ".to_string()
            } else if activity_count == 0 {
                "  ".to_string()
            } else {
                format!("{:02}", activity_count % 100)
            };

            let color = if current_date > today {
                Color::Reset
            } else {
                match activity_count {
                    0 => Color::Indexed(235),
                    1..=2 => Color::DarkGray,
                    3..=5 => Color::Green,
                    6..=9 => Color::LightGreen,
                    _ => Color::White,
                }
            };

            line_spans.push(Span::styled(
                display_text,
                Style::default().fg(Color::Black).bg(color),
            ));
            if week < WEEKS_TO_DISPLAY - 1 {
                line_spans.push(Span::raw(" "));
            }
        }
        lines.push(Line::from(line_spans));
    }

    // 3 = [[SPACE][SPACE](cell)][SPACE(margin)] + 2(margin)
    let content_width = label_width + (WEEKS_TO_DISPLAY * 3).saturating_sub(1) + 2;

    // 2 = border
    let widget_width = std::cmp::max(content_width, title.len() as u16) + 2;
    let centered_area = horizontal_centered_rect(widget_width, 9, area);

    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Plain),
            )
            .alignment(Alignment::Center),
        centered_area,
    );
}

fn horizontal_centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y,
        width.min(area.width),
        height.min(area.height),
    )
}

fn absolute_centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let center_y = area.y + area.height.saturating_sub(height) / 2;
    let center_x = area.x + area.width.saturating_sub(width) / 2;
    Rect::new(
        center_x,
        center_y,
        width.min(area.width),
        height.min(area.height),
    )
}
