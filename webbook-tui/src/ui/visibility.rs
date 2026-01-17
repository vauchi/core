//! Visibility Screen UI

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::app::App;

/// Draw the visibility settings screen.
pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Info
            Constraint::Min(0),     // Field list
        ])
        .split(area);

    // Get contact name
    let contact_name = if let Some(ref id) = app.visibility_state.contact_id {
        app.backend.list_contacts()
            .ok()
            .and_then(|contacts| {
                contacts.iter()
                    .find(|c| &c.id == id)
                    .map(|c| c.display_name.clone())
            })
            .unwrap_or_else(|| "Unknown".to_string())
    } else {
        "Unknown".to_string()
    };

    // Info section
    let info = Paragraph::new(format!("What {} can see of your card:", contact_name))
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(info, chunks[0]);

    // Field visibility list
    if let Some(ref contact_id) = app.visibility_state.contact_id {
        if let Ok(fields) = app.backend.get_contact_visibility(contact_id) {
            if fields.is_empty() {
                let empty = Paragraph::new("No fields on your card yet. Add fields to manage visibility.")
                    .style(Style::default().fg(Color::DarkGray))
                    .block(Block::default().borders(Borders::ALL).title("Fields"));
                f.render_widget(empty, chunks[1]);
            } else {
                let items: Vec<ListItem> = fields.iter().enumerate().map(|(i, field)| {
                    let icon = if field.can_see { "[X]" } else { "[ ]" };
                    let visibility = if field.can_see { "Visible" } else { "Hidden" };
                    let style = if i == app.visibility_state.selected_field {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else if field.can_see {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Red)
                    };
                    ListItem::new(format!("{} {} - {}", icon, field.field_label, visibility))
                        .style(style)
                }).collect();

                let mut state = ListState::default();
                state.select(Some(app.visibility_state.selected_field));

                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title("Fields"))
                    .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

                f.render_stateful_widget(list, chunks[1], &mut state);
            }
        } else {
            let error = Paragraph::new("Error loading visibility data")
                .style(Style::default().fg(Color::Red))
                .block(Block::default().borders(Borders::ALL).title("Error"));
            f.render_widget(error, chunks[1]);
        }
    }
}
