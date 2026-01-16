//! Keyboard Input Handling

use crossterm::event::KeyCode;

use crate::app::{AddFieldFocus, App, InputMode, Screen};
use crate::backend::{Backend, FIELD_TYPES};

/// Action to take after handling input.
pub enum Action {
    Continue,
    Quit,
}

/// Handle a key press.
pub fn handle_key(app: &mut App, key: KeyCode) -> Action {
    match app.input_mode {
        InputMode::Normal => handle_normal_mode(app, key),
        InputMode::Editing => handle_editing_mode(app, key),
    }
}

fn handle_normal_mode(app: &mut App, key: KeyCode) -> Action {
    // Global keys
    match key {
        KeyCode::Char('q') => return Action::Quit,
        KeyCode::Char('?') => {
            app.goto(Screen::Help);
            return Action::Continue;
        }
        KeyCode::Esc => {
            app.go_back();
            return Action::Continue;
        }
        _ => {}
    }

    // Screen-specific keys
    match app.screen {
        Screen::Home => handle_home_keys(app, key),
        Screen::Contacts => handle_contacts_keys(app, key),
        Screen::ContactDetail => handle_contact_detail_keys(app, key),
        Screen::Exchange => handle_exchange_keys(app, key),
        Screen::Settings => handle_settings_keys(app, key),
        Screen::Help => handle_help_keys(app, key),
        Screen::AddField => handle_add_field_keys(app, key),
    }

    Action::Continue
}

fn handle_editing_mode(app: &mut App, key: KeyCode) -> Action {
    match key {
        KeyCode::Esc => {
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Enter => {
            // Submit the input
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            match app.screen {
                Screen::AddField => {
                    match app.add_field_state.focus {
                        AddFieldFocus::Label => { app.add_field_state.label.pop(); }
                        AddFieldFocus::Value => { app.add_field_state.value.pop(); }
                        _ => {}
                    }
                }
                _ => { app.input_buffer.pop(); }
            }
        }
        KeyCode::Char(c) => {
            match app.screen {
                Screen::AddField => {
                    match app.add_field_state.focus {
                        AddFieldFocus::Label => app.add_field_state.label.push(c),
                        AddFieldFocus::Value => app.add_field_state.value.push(c),
                        _ => {}
                    }
                }
                _ => app.input_buffer.push(c),
            }
        }
        _ => {}
    }
    Action::Continue
}

fn handle_home_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('e') => app.goto(Screen::Exchange),
        KeyCode::Char('c') => app.goto(Screen::Contacts),
        KeyCode::Char('s') => app.goto(Screen::Settings),
        KeyCode::Char('a') => {
            app.add_field_state = Default::default();
            app.goto(Screen::AddField);
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let fields = app.backend.get_card_fields().unwrap_or_default();
            if app.selected_field < fields.len().saturating_sub(1) {
                app.selected_field += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.selected_field > 0 {
                app.selected_field -= 1;
            }
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            // Delete selected field
            if let Ok(fields) = app.backend.get_card_fields() {
                if let Some(field) = fields.get(app.selected_field) {
                    if app.backend.remove_field(&field.label).is_ok() {
                        app.set_status("Field removed");
                        if app.selected_field > 0 {
                            app.selected_field -= 1;
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn handle_contacts_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('j') | KeyCode::Down => {
            let count = app.backend.contact_count().unwrap_or(0);
            if app.selected_contact < count.saturating_sub(1) {
                app.selected_contact += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.selected_contact > 0 {
                app.selected_contact -= 1;
            }
        }
        KeyCode::Enter => {
            app.goto(Screen::ContactDetail);
        }
        _ => {}
    }
}

fn handle_contact_detail_keys(_app: &mut App, _key: KeyCode) {
    // Contact detail specific keys
}

fn handle_exchange_keys(_app: &mut App, _key: KeyCode) {
    // Exchange specific keys
}

fn handle_settings_keys(_app: &mut App, _key: KeyCode) {
    // Settings specific keys
}

fn handle_help_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter => {
            app.go_back();
        }
        _ => {}
    }
}

fn handle_add_field_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Tab => {
            // Cycle through fields
            app.add_field_state.focus = match app.add_field_state.focus {
                AddFieldFocus::Type => AddFieldFocus::Label,
                AddFieldFocus::Label => AddFieldFocus::Value,
                AddFieldFocus::Value => AddFieldFocus::Type,
            };
            app.input_mode = if app.add_field_state.focus == AddFieldFocus::Type {
                InputMode::Normal
            } else {
                InputMode::Editing
            };
        }
        KeyCode::Enter => {
            if app.add_field_state.focus == AddFieldFocus::Value {
                // Submit the field
                let field_type = Backend::parse_field_type(
                    FIELD_TYPES[app.add_field_state.field_type_index]
                );
                if let Err(e) = app.backend.add_field(
                    field_type,
                    &app.add_field_state.label,
                    &app.add_field_state.value,
                ) {
                    app.set_status(format!("Error: {}", e));
                } else {
                    app.set_status("Field added");
                    app.go_back();
                }
            } else {
                // Move to next field
                app.add_field_state.focus = match app.add_field_state.focus {
                    AddFieldFocus::Type => {
                        app.input_mode = InputMode::Editing;
                        AddFieldFocus::Label
                    }
                    AddFieldFocus::Label => AddFieldFocus::Value,
                    AddFieldFocus::Value => AddFieldFocus::Value,
                };
            }
        }
        KeyCode::Left | KeyCode::Char('h') if app.add_field_state.focus == AddFieldFocus::Type => {
            if app.add_field_state.field_type_index > 0 {
                app.add_field_state.field_type_index -= 1;
            }
        }
        KeyCode::Right | KeyCode::Char('l') if app.add_field_state.focus == AddFieldFocus::Type => {
            if app.add_field_state.field_type_index < FIELD_TYPES.len() - 1 {
                app.add_field_state.field_type_index += 1;
            }
        }
        _ => {}
    }
}
