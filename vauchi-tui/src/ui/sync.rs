//! Sync Status Screen

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem, Paragraph};

use crate::app::App;

/// Draw the sync screen.
pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6), // Connection status
            Constraint::Length(5), // Sync progress
            Constraint::Min(0),    // Sync log
        ])
        .split(area);

    // Connection status - use real values from backend
    let relay_url = app.backend.relay_url();
    let sync_state = &app.sync_state;

    let status_style = if sync_state.connected {
        Style::default().fg(Color::Green)
    } else if sync_state.is_syncing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let status_text = if sync_state.is_syncing {
        "Syncing..."
    } else if sync_state.connected {
        "Connected"
    } else {
        "Disconnected"
    };

    let status_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  Relay: "),
            Span::styled(relay_url, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw("  Status: "),
            Span::styled(status_text, status_style),
        ]),
        Line::from(""),
    ];

    let status = Paragraph::new(status_lines)
        .block(Block::default().borders(Borders::ALL).title("Connection"));
    f.render_widget(status, chunks[0]);

    // Sync progress
    let contact_count = app.backend.contact_count().unwrap_or(0);
    let pending_updates = sync_state.pending_updates;

    let progress_label = if pending_updates > 0 {
        format!(
            "{} contacts | {} pending updates",
            contact_count, pending_updates
        )
    } else {
        format!("{} contacts synced", contact_count)
    };

    // Calculate progress ratio
    let progress_ratio = if sync_state.is_syncing {
        0.5 // Show partial progress while syncing
    } else if pending_updates > 0 {
        0.7 // Mostly synced but has pending
    } else {
        1.0 // Fully synced
    };

    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Sync Progress"),
        )
        .gauge_style(Style::default().fg(Color::Cyan))
        .ratio(progress_ratio)
        .label(progress_label);

    f.render_widget(gauge, chunks[1]);

    // Sync log / instructions
    let mut log_items: Vec<ListItem> = Vec::new();

    // Add last result if available
    if let Some(ref result) = sync_state.last_result {
        log_items.push(ListItem::new(Line::from(vec![
            Span::styled("  Last sync: ", Style::default().fg(Color::Cyan)),
            Span::raw(result.as_str()),
        ])));
        log_items.push(ListItem::new(""));
    }

    // Add sync log entries
    for entry in sync_state.sync_log.iter().rev().take(5) {
        log_items.push(ListItem::new(Span::styled(
            format!("  {}", entry),
            Style::default().fg(Color::DarkGray),
        )));
    }

    if log_items.is_empty() || sync_state.last_result.is_none() {
        log_items.push(ListItem::new(""));
        log_items.push(ListItem::new(Span::styled(
            "  Press [s] to start sync",
            Style::default().fg(Color::Yellow),
        )));
        log_items.push(ListItem::new(""));
        log_items.push(ListItem::new(Span::styled(
            "  Sync Operations:",
            Style::default().fg(Color::Cyan),
        )));
        log_items.push(ListItem::new("  - Connect to relay server"));
        log_items.push(ListItem::new("  - Receive pending exchange messages"));
        log_items.push(ListItem::new("  - Process contact card updates"));
        log_items.push(ListItem::new("  - Send outbound updates to contacts"));
        log_items.push(ListItem::new(""));
        log_items.push(ListItem::new(Span::styled(
            "  Press [t] to test relay connection",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let log_list =
        List::new(log_items).block(Block::default().borders(Borders::ALL).title("Sync Info"));
    f.render_widget(log_list, chunks[2]);
}
