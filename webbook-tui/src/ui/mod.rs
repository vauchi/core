//! UI Rendering

mod home;
mod contacts;
mod exchange;
mod settings;
mod help;
mod widgets;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, Screen};

/// Draw the application.
pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(0),     // Content
            Constraint::Length(3),  // Footer/status
        ])
        .split(f.area());

    // Header
    draw_header(f, chunks[0], app);

    // Content
    match app.screen {
        Screen::Home => home::draw(f, chunks[1], app),
        Screen::Contacts => contacts::draw(f, chunks[1], app),
        Screen::ContactDetail => contacts::draw_detail(f, chunks[1], app),
        Screen::Exchange => exchange::draw(f, chunks[1], app),
        Screen::Settings => settings::draw(f, chunks[1], app),
        Screen::Help => help::draw(f, chunks[1], app),
        Screen::AddField => home::draw_add_field(f, chunks[1], app),
    }

    // Footer
    draw_footer(f, chunks[2], app);
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let title = match app.screen {
        Screen::Home => "WebBook",
        Screen::Contacts => "Contacts",
        Screen::ContactDetail => "Contact Details",
        Screen::Exchange => "Exchange",
        Screen::Settings => "Settings",
        Screen::Help => "Help",
        Screen::AddField => "Add Field",
    };

    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::BOTTOM));

    f.render_widget(header, area);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let help_text = match app.screen {
        Screen::Home => "[e]xchange  [c]ontacts  [s]ettings  [a]dd field  [?]help  [q]uit",
        Screen::Contacts => "[j/k] navigate  [enter] view  [esc] back  [?]help",
        Screen::ContactDetail => "[esc] back  [?]help",
        Screen::Exchange => "[esc] back  [?]help",
        Screen::Settings => "[esc] back  [?]help",
        Screen::Help => "[esc/q] close",
        Screen::AddField => "[tab] next  [enter] submit  [esc] cancel",
    };

    let status = if let Some(msg) = &app.status_message {
        format!("{} | {}", msg, help_text)
    } else {
        help_text.to_string()
    };

    let footer = Paragraph::new(status)
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));

    f.render_widget(footer, area);
}
