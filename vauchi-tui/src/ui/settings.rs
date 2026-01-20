//! Settings Screen

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, InputMode};

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Display name
            Constraint::Length(4), // Public ID
            Constraint::Length(4), // Relay URL
            Constraint::Min(0),    // Options
        ])
        .margin(1)
        .split(area);

    // Display name (editable)
    let name = app.backend.display_name().unwrap_or("Not set");
    let name_para = Paragraph::new(format!("Display Name: {}  [press n to edit]", name))
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().title("Identity").borders(Borders::ALL));
    f.render_widget(name_para, chunks[0]);

    // Public ID
    if let Some(id) = app.backend.public_id() {
        let id_para = Paragraph::new(format!("Public ID: {}", id))
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(id_para, chunks[1]);
    }

    // Relay URL
    let relay_url = app.backend.relay_url();
    let relay_para = Paragraph::new(format!("Relay: {}  [press u to edit]", relay_url))
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().title("Sync Server").borders(Borders::ALL));
    f.render_widget(relay_para, chunks[2]);

    // Options and Help Links
    let options = "\
Options:
  [n] Edit display name
  [u] Edit relay URL
  [b] Backup & restore
  [d] Device management
  [r] Recovery settings

Help & Support:
  User Guide:     https://vauchi.app/user-guide
  FAQ:            https://vauchi.app/faq
  Report Issue:   https://github.com/vauchi/issues
  Privacy Policy: https://vauchi.app/privacy

Settings are automatically saved.
Your identity is stored locally and encrypted.

Version 1.0.0
";

    let help_para = Paragraph::new(options)
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(help_para, chunks[3]);
}

pub fn draw_edit_name(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Current name info
            Constraint::Length(3), // Name input
            Constraint::Min(0),    // Spacer
        ])
        .margin(2)
        .split(area);

    let state = &app.edit_name_state;

    // Current name info
    let current_name = app.backend.display_name().unwrap_or("Not set");
    let info_para = Paragraph::new(format!("Current: {}", current_name))
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(info_para, chunks[0]);

    // Name input
    let name_text = if app.input_mode == InputMode::Editing {
        format!("{}|", state.new_name)
    } else {
        state.new_name.clone()
    };
    let name_para = Paragraph::new(name_text)
        .style(Style::default().fg(Color::Yellow))
        .block(
            Block::default()
                .title("New Display Name")
                .borders(Borders::ALL),
        );
    f.render_widget(name_para, chunks[1]);
}

pub fn draw_edit_relay_url(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Current URL info
            Constraint::Length(3), // URL input
            Constraint::Length(3), // Help text
            Constraint::Min(0),    // Spacer
        ])
        .margin(2)
        .split(area);

    let state = &app.edit_relay_url_state;

    // Current URL info
    let current_url = app.backend.relay_url();
    let info_para = Paragraph::new(format!("Current: {}", current_url))
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(info_para, chunks[0]);

    // URL input
    let url_text = if app.input_mode == InputMode::Editing {
        format!("{}|", state.new_url)
    } else {
        state.new_url.clone()
    };
    let url_para = Paragraph::new(url_text)
        .style(Style::default().fg(Color::Cyan))
        .block(
            Block::default()
                .title("New Relay URL")
                .borders(Borders::ALL),
        );
    f.render_widget(url_para, chunks[1]);

    // Help text
    let help_para = Paragraph::new("URL must start with wss:// (or ws:// for local dev)")
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(help_para, chunks[2]);
}
