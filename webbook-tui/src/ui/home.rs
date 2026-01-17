//! Home Screen

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::app::{AddFieldFocus, App, InputMode};
use crate::backend::FIELD_TYPES;

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Welcome
            Constraint::Length(3), // Public ID
            Constraint::Min(0),    // Fields
            Constraint::Length(2), // Contact count
        ])
        .split(area);

    // Welcome message
    let name = app.backend.display_name().unwrap_or("Guest");
    let welcome = if app.backend.has_identity() {
        format!("Hello, {}!", name)
    } else {
        "Welcome to WebBook!".to_string()
    };

    let welcome_para = Paragraph::new(welcome)
        .style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(welcome_para, chunks[0]);

    // Public ID
    if let Some(id) = app.backend.public_id() {
        let id_para = Paragraph::new(format!("Public ID: {}", id))
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(id_para, chunks[1]);
    }

    // Card fields
    let fields = app.backend.get_card_fields().unwrap_or_default();

    if fields.is_empty() {
        let empty = Paragraph::new("No fields yet. Press 'a' to add contact info!")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, chunks[2]);
    } else {
        let items: Vec<ListItem> = fields
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let icon = match field.field_type.as_str() {
                    "Email" => "📧",
                    "Phone" => "📱",
                    "Website" => "🌐",
                    "Address" => "📍",
                    "Social" => "🔗",
                    _ => "📝",
                };

                let content = format!(
                    "{} {} ({})  {}",
                    icon, field.label, field.field_type, field.value
                );
                let style = if i == app.selected_field {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(content).style(style)
            })
            .collect();

        let list =
            List::new(items).block(Block::default().title("Your Card").borders(Borders::ALL));

        let mut state = ListState::default();
        state.select(Some(app.selected_field));
        f.render_stateful_widget(list, chunks[2], &mut state);
    }

    // Contact count
    let count = app.backend.contact_count().unwrap_or(0);
    let count_para =
        Paragraph::new(format!("Contacts: {}", count)).style(Style::default().fg(Color::DarkGray));
    f.render_widget(count_para, chunks[3]);
}

pub fn draw_add_field(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Type selector
            Constraint::Length(3), // Label input
            Constraint::Length(3), // Value input
            Constraint::Min(0),    // Spacer
        ])
        .margin(2)
        .split(area);

    let state = &app.add_field_state;

    // Type selector
    let type_text = format!("< {} >", FIELD_TYPES[state.field_type_index]);
    let type_style = if state.focus == AddFieldFocus::Type {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let type_para = Paragraph::new(type_text)
        .style(type_style)
        .block(Block::default().title("Type").borders(Borders::ALL));
    f.render_widget(type_para, chunks[0]);

    // Label input
    let label_style = if state.focus == AddFieldFocus::Label {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let label_text = if state.label.is_empty() && state.focus != AddFieldFocus::Label {
        "Enter label...".to_string()
    } else if state.focus == AddFieldFocus::Label && app.input_mode == InputMode::Editing {
        format!("{}|", state.label)
    } else {
        state.label.clone()
    };
    let label_para = Paragraph::new(label_text)
        .style(label_style)
        .block(Block::default().title("Label").borders(Borders::ALL));
    f.render_widget(label_para, chunks[1]);

    // Value input
    let value_style = if state.focus == AddFieldFocus::Value {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let value_text = if state.value.is_empty() && state.focus != AddFieldFocus::Value {
        "Enter value...".to_string()
    } else if state.focus == AddFieldFocus::Value && app.input_mode == InputMode::Editing {
        format!("{}|", state.value)
    } else {
        state.value.clone()
    };
    let value_para = Paragraph::new(value_text)
        .style(value_style)
        .block(Block::default().title("Value").borders(Borders::ALL));
    f.render_widget(value_para, chunks[2]);
}
