use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table};
use ratatui::Frame;

use crate::app::{App, Panel};

pub fn draw(f: &mut Frame, app: &App) {
    let size = f.area();
    if let Some(preview) = &app.preview {
        let mut lines = vec![
            Line::raw(preview.policy.clone()),
            Line::raw(format!("Repository: {}", preview.repository)),
            Line::raw(format!(
                "Branch: {}  Remote: {}  Auto push: {}",
                preview.branch.as_deref().unwrap_or("unknown"),
                preview.remote.as_deref().unwrap_or("unknown"),
                preview
                    .auto_push
                    .map(|v| if v { "yes" } else { "no" })
                    .unwrap_or("unknown")
            )),
        ];
        lines.extend(
            preview
                .blockers
                .iter()
                .map(|b| Line::raw(format!("Blocked: {b}"))),
        );
        lines.extend(preview.entries.iter().map(|e| {
            Line::raw(format!(
                "{} {} (staged: {}, unstaged: {})",
                e.change, e.path, e.staged, e.unstaged
            ))
        }));
        f.render_widget(
            Paragraph::new(lines)
                .wrap(ratatui::widgets::Wrap { trim: false })
                .scroll((app.preview_scroll as u16, 0))
                .block(panel_block(
                    " Changes • Esc: dashboard • ↑↓: scroll • r: refresh ",
                    true,
                )),
            size,
        );
        return;
    }

    // Main layout: body + footer
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(1)])
        .split(size);

    // Body: top row (60%) + bottom row (40%)
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(outer[0]);

    // Top: Status | Logs
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(rows[0]);

    // Bottom: Config | Providers
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    draw_status(f, app, top[0]);
    draw_logs(f, app, top[1]);
    draw_config(f, app, bottom[0]);
    draw_providers(f, app, bottom[1]);
    draw_footer(f, app, outer[1]);
}

fn panel_block(title: &str, active: bool) -> Block<'_> {
    let style = if active {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(style)
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let active = app.active_panel == Panel::Status;
    let block = panel_block(" Status ", active);

    let state_indicator = if app.running {
        Span::styled("● Running", Style::default().fg(Color::Green))
    } else {
        Span::styled("○ Stopped", Style::default().fg(Color::Red))
    };

    let enabled_text = if app.enabled { "yes" } else { "no" };

    let changes_text = if app.changes.is_empty() {
        "No pending changes".to_string()
    } else {
        format!(
            "{} pending ({})",
            app.changes.total(),
            app.changes.to_summary_text()
        )
    };

    let last_commit_text = app.last_commit.as_deref().unwrap_or("never");

    let mut lines = vec![
        Line::from(vec![Span::raw("  State:    "), state_indicator]),
        Line::from(format!("  Enabled:  {}", enabled_text)),
        Line::from(format!("  Schedule: {}", app.schedule_desc)),
        Line::from(format!("  Branch:   {}", app.current_branch)),
        Line::from(format!("  Last:     {}", last_commit_text)),
        Line::raw(""),
        Line::from(format!("  Changes:  {}", changes_text)),
    ];
    if let Some(error) = &app.action_error {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!("  Action failed: {error}"),
            Style::default().fg(Color::Red),
        )));
    }

    if let Some(status) = &app.repository_status {
        lines = vec![Line::raw(format!(
            "Scheduler: {} • {}",
            if app.running { "running" } else { "stopped" },
            app.schedule_desc
        ))];
        lines.extend(status.lines().into_iter().map(Line::raw));
        if let Some(error) = &app.action_error {
            lines.push(Line::raw(format!("Action failed: {error}")));
        }
    }
    let paragraph = Paragraph::new(lines)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .scroll((app.status_scroll as u16, 0))
        .block(block);
    f.render_widget(paragraph, area);
}

fn draw_logs(f: &mut Frame, app: &App, area: Rect) {
    let active = app.active_panel == Panel::Logs;
    let title = format!(" Logs ({}) ", app.log_lines.len());
    let block = panel_block(&title, active);

    if app.log_lines.is_empty() {
        let paragraph = Paragraph::new("  No log entries yet")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        f.render_widget(paragraph, area);
        return;
    }

    let inner_height = area.height.saturating_sub(2) as usize; // block borders

    let items: Vec<ListItem> = app
        .log_lines
        .iter()
        .skip(app.log_scroll)
        .take(inner_height)
        .map(|entry| {
            let level_color = match entry.level.as_str() {
                "ERROR" => Color::Red,
                "WARN" => Color::Yellow,
                "INFO" => Color::Green,
                "DEBUG" => Color::DarkGray,
                _ => Color::White,
            };

            let line = Line::from(vec![
                Span::styled(
                    format!(" {:5} ", entry.level),
                    Style::default().fg(level_color),
                ),
                Span::styled(
                    format!("{} ", entry.timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(&entry.message),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_config(f: &mut Frame, app: &App, area: Rect) {
    let active = app.active_panel == Panel::Config;
    let block = panel_block(" Config ", active);

    if app
        .repository_status
        .as_ref()
        .is_some_and(|s| s.schedule.is_none())
    {
        f.render_widget(
            Paragraph::new("Configuration unavailable. See status diagnostics.").block(block),
            area,
        );
        return;
    }
    let lines = vec![
        Line::from(format!("  schedule:      {}", app.schedule)),
        Line::from(format!("  auto_push:     {}", app.auto_push)),
        Line::from(format!("  branch:        {}", app.branch)),
        Line::from(format!("  log_level:     {}", app.log_level)),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}

fn draw_providers(f: &mut Frame, app: &App, area: Rect) {
    let active = app.active_panel == Panel::Providers;
    let block = panel_block(" Providers ", active);

    if app.providers.is_empty() {
        let paragraph = Paragraph::new("  AI commit messages disabled")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        f.render_widget(paragraph, area);
        return;
    }

    let rows: Vec<Row> = app
        .providers
        .iter()
        .map(|(_, name, available)| {
            let (indicator, color) = if *available {
                ("●", Color::Green)
            } else {
                ("○", Color::Red)
            };
            let status = if *available { "OK" } else { "N/A" };

            Row::new(vec![
                format!("  {} {}", indicator, name),
                status.to_string(),
            ])
            .style(Style::default().fg(color))
        })
        .collect();

    let widths = [Constraint::Percentage(70), Constraint::Percentage(30)];
    let table = Table::new(rows, widths).block(block);
    f.render_widget(table, area);
}

fn draw_footer(f: &mut Frame, _app: &App, area: Rect) {
    let keys = Line::from(vec![
        Span::styled(
            " Tab",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(":panel  "),
        Span::styled(
            "q",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(":quit  "),
        Span::styled(
            "s",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(":start/stop  "),
        Span::styled(
            "r",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(":refresh  "),
        Span::styled(
            "↑↓",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(":scroll status/logs  p:preview"),
    ]);

    let paragraph = Paragraph::new(keys).style(Style::default().fg(Color::White));
    f.render_widget(paragraph, area);
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
