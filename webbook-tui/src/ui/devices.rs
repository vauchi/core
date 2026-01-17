//! Device Management Screen

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::app::App;

/// Draw the devices screen.
pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Info
            Constraint::Min(0),     // Device list
        ])
        .split(area);

    // Device info
    let info_text = if app.backend.has_identity() {
        vec![
            Line::from(""),
            Line::from(Span::styled("Current Device", Style::default().fg(Color::Cyan))),
            Line::from(""),
        ]
    } else {
        vec![
            Line::from(""),
            Line::from(Span::styled("No identity configured", Style::default().fg(Color::Yellow))),
        ]
    };

    let info = Paragraph::new(info_text)
        .block(Block::default().borders(Borders::ALL).title("Device Info"));
    f.render_widget(info, chunks[0]);

    // Device list (placeholder - would need backend support)
    let devices: Vec<ListItem> = vec![
        ListItem::new(Line::from(vec![
            Span::styled("1. ", Style::default().fg(Color::DarkGray)),
            Span::raw("This Device"),
            Span::styled(" [active]", Style::default().fg(Color::Green)),
        ])),
    ];

    let device_list = List::new(devices)
        .block(Block::default().borders(Borders::ALL).title("Linked Devices"))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_widget(device_list, chunks[1]);
}
