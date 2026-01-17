//! Sync Status Screen

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};

use crate::app::App;

/// Draw the sync screen.
pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),  // Connection status
            Constraint::Length(5),  // Sync progress
            Constraint::Min(0),     // Sync log
        ])
        .split(area);

    // Connection status
    let relay_url = "ws://localhost:8080"; // Would come from config
    let connected = false; // Would come from backend state

    let status_style = if connected {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Yellow)
    };

    let status_text = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  Relay: "),
            Span::styled(relay_url, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw("  Status: "),
            Span::styled(
                if connected { "Connected" } else { "Disconnected" },
                status_style
            ),
        ]),
        Line::from(""),
    ];

    let status = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::ALL).title("Connection"));
    f.render_widget(status, chunks[0]);

    // Sync progress
    let contact_count = app.backend.contact_count().unwrap_or(0);
    let progress_label = format!("{} contacts synced", contact_count);

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title("Sync Progress"))
        .gauge_style(Style::default().fg(Color::Cyan))
        .ratio(1.0) // Would be actual sync progress
        .label(progress_label);

    f.render_widget(gauge, chunks[1]);

    // Sync info/log
    let log_text = vec![
        Line::from(""),
        Line::from(Span::styled("  Press [s] to start sync", Style::default().fg(Color::Yellow))),
        Line::from(""),
        Line::from(Span::styled("  Sync Operations:", Style::default().fg(Color::Cyan))),
        Line::from("  - Push local changes to relay"),
        Line::from("  - Pull updates from contacts"),
        Line::from("  - Resolve any conflicts"),
        Line::from(""),
        Line::from(Span::styled("  Use CLI for manual sync:", Style::default().fg(Color::DarkGray))),
        Line::from(Span::styled("  webbook sync", Style::default().fg(Color::DarkGray))),
        Line::from(""),
    ];

    let log = Paragraph::new(log_text)
        .block(Block::default().borders(Borders::ALL).title("Sync Info"));
    f.render_widget(log, chunks[2]);
}
