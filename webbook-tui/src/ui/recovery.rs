//! Recovery Screen

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;

/// Draw the recovery screen.
pub fn draw(f: &mut Frame, area: Rect, _app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),  // Status
            Constraint::Length(8),  // Actions
            Constraint::Min(0),     // Info
        ])
        .split(area);

    // Recovery status
    let status_text = vec![
        Line::from(""),
        Line::from(Span::styled("Recovery Status", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from("  No recovery in progress"),
        Line::from(""),
    ];

    let status = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(status, chunks[0]);

    // Recovery actions
    let actions_text = vec![
        Line::from(""),
        Line::from(Span::styled("  [c]", Style::default().fg(Color::Yellow))),
        Line::from("    Create recovery claim"),
        Line::from(Span::styled("  [v]", Style::default().fg(Color::Yellow))),
        Line::from("    Vouch for a contact"),
        Line::from(""),
    ];

    let actions = Paragraph::new(actions_text)
        .block(Block::default().borders(Borders::ALL).title("Actions"));
    f.render_widget(actions, chunks[1]);

    // Recovery info
    let info_text = vec![
        Line::from(""),
        Line::from(Span::styled("How Recovery Works", Style::default().fg(Color::Cyan))),
        Line::from(""),
        Line::from("  1. Lost your device? Create a new identity"),
        Line::from("  2. Generate a recovery claim with your OLD public key"),
        Line::from("  3. Ask 3+ contacts to vouch for you in person"),
        Line::from("  4. Collect vouchers to prove your identity"),
        Line::from("  5. Share your recovery proof with all contacts"),
        Line::from(""),
        Line::from(Span::styled("  Use CLI for full recovery workflow:", Style::default().fg(Color::DarkGray))),
        Line::from(Span::styled("  webbook recovery --help", Style::default().fg(Color::DarkGray))),
        Line::from(""),
    ];

    let info = Paragraph::new(info_text)
        .block(Block::default().borders(Borders::ALL).title("Information"));
    f.render_widget(info, chunks[2]);
}
