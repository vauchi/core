//! Contacts Screen

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::app::App;

/// Filter contacts based on search query.
fn filter_contacts<'a>(
    contacts: &'a [crate::backend::ContactInfo],
    query: &str,
) -> Vec<(usize, &'a crate::backend::ContactInfo)> {
    if query.is_empty() {
        contacts.iter().enumerate().collect()
    } else {
        let query = query.to_lowercase();
        contacts
            .iter()
            .enumerate()
            .filter(|(_, c)| c.display_name.to_lowercase().contains(&query))
            .collect()
    }
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let contacts = app.backend.list_contacts().unwrap_or_default();

    // Split area for search bar and list
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    // Search bar
    let search_title = if app.contact_search_mode {
        "Search (type to search, Esc to exit)"
    } else {
        "Search (/ to search)"
    };
    let search_style = if app.contact_search_mode {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let search_text = if app.contact_search_query.is_empty() && !app.contact_search_mode {
        "Press / to search...".to_string()
    } else {
        app.contact_search_query.clone()
    };
    let search_bar = Paragraph::new(search_text)
        .style(search_style)
        .block(Block::default().borders(Borders::ALL).title(search_title));
    f.render_widget(search_bar, chunks[0]);

    // Filter contacts
    let filtered = filter_contacts(&contacts, &app.contact_search_query);

    if filtered.is_empty() {
        let msg = if contacts.is_empty() {
            "No contacts yet. Exchange cards to add contacts!"
        } else {
            "No contacts match your search."
        };
        let empty = Paragraph::new(msg)
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title("Contacts"));
        f.render_widget(empty, chunks[1]);
        return;
    }

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(display_idx, (_, contact))| {
            let verified = if contact.verified { "✓" } else { " " };
            let content = format!(
                "[{}] {}  ({}...)",
                verified,
                contact.display_name,
                &contact.id[..8]
            );
            let style = if display_idx == app.selected_contact {
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
                .title(format!("Contacts ({} shown)", filtered.len()))
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = ListState::default();
    state.select(Some(app.selected_contact));
    f.render_stateful_widget(list, chunks[1], &mut state);
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
