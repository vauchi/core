//! Contacts Screen

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::app::App;

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let contacts = app.backend.list_contacts().unwrap_or_default();

    if contacts.is_empty() {
        let empty = Paragraph::new("No contacts yet. Exchange cards to add contacts!")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title("Contacts"));
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = contacts
        .iter()
        .enumerate()
        .map(|(i, contact)| {
            let verified = if contact.verified { "✓" } else { " " };
            let content = format!(
                "[{}] {}  ({}...)",
                verified,
                contact.display_name,
                &contact.id[..8]
            );
            let style = if i == app.selected_contact {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(content).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().title("Contacts").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = ListState::default();
    state.select(Some(app.selected_contact));
    f.render_stateful_widget(list, area, &mut state);
}

pub fn draw_detail(f: &mut Frame, area: Rect, app: &App) {
    let contacts = app.backend.list_contacts().unwrap_or_default();
    let contact = contacts.get(app.selected_contact);

    match contact {
        Some(c) => {
            let fields = app
                .backend
                .get_contact_fields(app.selected_contact)
                .unwrap_or_default();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // Name
                    Constraint::Length(3), // Status
                    Constraint::Min(5),    // Fields
                    Constraint::Length(2), // Help
                ])
                .split(area);

            let name = Paragraph::new(c.display_name.clone())
                .style(Style::default().add_modifier(Modifier::BOLD))
                .block(Block::default().title("Name").borders(Borders::ALL));
            f.render_widget(name, chunks[0]);

            let verified_text = if c.verified {
                "Verified ✓"
            } else {
                "Not verified"
            };
            let verified_style = if c.verified {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Yellow)
            };
            let verified = Paragraph::new(verified_text)
                .style(verified_style)
                .block(Block::default().title("Status").borders(Borders::ALL));
            f.render_widget(verified, chunks[1]);

            // Fields list with selection
            if fields.is_empty() {
                let empty = Paragraph::new("No contact info shared")
                    .style(Style::default().fg(Color::DarkGray))
                    .block(Block::default().title("Contact Info").borders(Borders::ALL));
                f.render_widget(empty, chunks[2]);
            } else {
                let items: Vec<ListItem> = fields
                    .iter()
                    .enumerate()
                    .map(|(i, field)| {
                        let action_icon = match field.action_type.as_str() {
                            "call" => "📞",
                            "sms" => "💬",
                            "email" => "✉️",
                            "web" => "🌐",
                            "map" => "📍",
                            _ => "📋",
                        };
                        let content = format!("{} {}: {}", action_icon, field.label, field.value);
                        let style = if i == app.selected_contact_field {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        };
                        ListItem::new(content).style(style)
                    })
                    .collect();

                let list = List::new(items)
                    .block(
                        Block::default()
                            .title("Contact Info (j/k to navigate, Enter to open)")
                            .borders(Borders::ALL),
                    )
                    .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

                let mut state = ListState::default();
                state.select(Some(app.selected_contact_field));
                f.render_stateful_widget(list, chunks[2], &mut state);
            }

            // Help line
            let help = Paragraph::new("v=visibility  x=delete  o/Enter=open  Esc=back")
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(help, chunks[3]);
        }
        None => {
            let empty = Paragraph::new("Contact not found").style(Style::default().fg(Color::Red));
            f.render_widget(empty, area);
        }
    }
}
