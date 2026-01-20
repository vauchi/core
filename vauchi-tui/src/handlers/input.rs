//! Keyboard Input Handling

use crossterm::event::KeyCode;

use crate::app::{
    AddFieldFocus, App, BackupFocus, BackupMode, EditFieldState, EditNameState, EditRelayUrlState,
    InputMode, Screen,
};
use crate::backend::{Backend, FIELD_TYPES};
use vauchi_core::identity::password::validate_password;

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
    // Don't process global keys if in contact search mode
    if app.contact_search_mode && app.screen == Screen::Contacts {
        handle_contacts_keys(app, key);
        return Action::Continue;
    }

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
        Screen::ContactVisibility => handle_visibility_keys(app, key),
        Screen::Exchange => handle_exchange_keys(app, key),
        Screen::Settings => handle_settings_keys(app, key),
        Screen::Help => handle_help_keys(app, key),
        Screen::AddField => handle_add_field_keys(app, key),
        Screen::EditField => handle_edit_field_keys(app, key),
        Screen::EditName => handle_edit_name_keys(app, key),
        Screen::EditRelayUrl => handle_edit_relay_url_keys(app, key),
        Screen::Devices => handle_devices_keys(app, key),
        Screen::Recovery => handle_recovery_keys(app, key),
        Screen::Sync => handle_sync_keys(app, key),
        Screen::Backup => handle_backup_keys(app, key),
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
        KeyCode::Backspace => match app.screen {
            Screen::AddField => match app.add_field_state.focus {
                AddFieldFocus::Label => {
                    app.add_field_state.label.pop();
                }
                AddFieldFocus::Value => {
                    app.add_field_state.value.pop();
                }
                _ => {}
            },
            Screen::EditField => {
                app.edit_field_state.new_value.pop();
            }
            Screen::EditName => {
                app.edit_name_state.new_name.pop();
            }
            Screen::EditRelayUrl => {
                app.edit_relay_url_state.new_url.pop();
            }
            Screen::Backup => match app.backup_state.focus {
                BackupFocus::Password => {
                    app.backup_state.password.pop();
                }
                BackupFocus::Confirm => {
                    app.backup_state.confirm_password.pop();
                }
                BackupFocus::Data => {
                    app.backup_state.backup_data.pop();
                }
            },
            _ => {
                app.input_buffer.pop();
            }
        },
        KeyCode::Char(c) => match app.screen {
            Screen::AddField => match app.add_field_state.focus {
                AddFieldFocus::Label => app.add_field_state.label.push(c),
                AddFieldFocus::Value => app.add_field_state.value.push(c),
                _ => {}
            },
            Screen::EditField => app.edit_field_state.new_value.push(c),
            Screen::EditName => app.edit_name_state.new_name.push(c),
            Screen::EditRelayUrl => app.edit_relay_url_state.new_url.push(c),
            Screen::Backup => match app.backup_state.focus {
                BackupFocus::Password => app.backup_state.password.push(c),
                BackupFocus::Confirm => app.backup_state.confirm_password.push(c),
                BackupFocus::Data => app.backup_state.backup_data.push(c),
            },
            _ => app.input_buffer.push(c),
        },
        _ => {}
    }
    Action::Continue
}

fn handle_home_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('c') => app.goto(Screen::Contacts),
        KeyCode::Char('s') => app.goto(Screen::Settings),
        KeyCode::Char('d') => app.goto(Screen::Devices),
        KeyCode::Char('r') => app.goto(Screen::Recovery),
        KeyCode::Char('n') => app.goto(Screen::Sync),
        KeyCode::Char('b') => app.goto(Screen::Backup),
        KeyCode::Char('a') => {
            app.add_field_state = Default::default();
            app.goto(Screen::AddField);
        }
        KeyCode::Char('e') | KeyCode::Enter => {
            // Edit selected field
            if let Ok(fields) = app.backend.get_card_fields() {
                if let Some(field) = fields.get(app.selected_field) {
                    app.edit_field_state = EditFieldState {
                        field_label: field.label.clone(),
                        field_type: field.field_type.clone(),
                        new_value: field.value.clone(),
                    };
                    app.goto(Screen::EditField);
                    app.input_mode = InputMode::Editing;
                } else {
                    // No fields, open Exchange
                    app.goto(Screen::Exchange);
                }
            }
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
        KeyCode::Char('x') | KeyCode::Delete => {
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
    // Handle search mode
    if app.contact_search_mode {
        match key {
            KeyCode::Esc => {
                app.contact_search_mode = false;
            }
            KeyCode::Enter => {
                app.contact_search_mode = false;
            }
            KeyCode::Backspace => {
                app.contact_search_query.pop();
                app.selected_contact = 0;
            }
            KeyCode::Char(c) => {
                app.contact_search_query.push(c);
                app.selected_contact = 0;
            }
            _ => {}
        }
        return;
    }

    // Normal navigation mode
    match key {
        KeyCode::Char('/') => {
            app.contact_search_mode = true;
            app.contact_search_query.clear();
            app.selected_contact = 0;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            // Count filtered contacts
            let contacts = app.backend.list_contacts().unwrap_or_default();
            let filtered_count = if app.contact_search_query.is_empty() {
                contacts.len()
            } else {
                let query = app.contact_search_query.to_lowercase();
                contacts
                    .iter()
                    .filter(|c| c.display_name.to_lowercase().contains(&query))
                    .count()
            };
            if app.selected_contact < filtered_count.saturating_sub(1) {
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

fn handle_contact_detail_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('j') | KeyCode::Down => {
            // Navigate down through contact fields
            if let Ok(fields) = app.backend.get_contact_fields(app.selected_contact) {
                if app.selected_contact_field < fields.len().saturating_sub(1) {
                    app.selected_contact_field += 1;
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            // Navigate up through contact fields
            if app.selected_contact_field > 0 {
                app.selected_contact_field -= 1;
            }
        }
        KeyCode::Enter | KeyCode::Char('o') => {
            // Open the selected field in external app
            match app
                .backend
                .open_contact_field(app.selected_contact, app.selected_contact_field)
            {
                Ok(msg) => app.set_status(msg),
                Err(e) => app.set_status(format!("Error: {}", e)),
            }
        }
        KeyCode::Char('v') => {
            // Open visibility settings for this contact
            if let Ok(Some(contact)) = app.backend.get_contact_by_index(app.selected_contact) {
                app.visibility_state.contact_id = Some(contact.id);
                app.visibility_state.selected_field = 0;
                app.goto(Screen::ContactVisibility);
            }
        }
        KeyCode::Char('x') | KeyCode::Delete => {
            // Delete contact
            if let Ok(contacts) = app.backend.list_contacts() {
                if let Some(contact) = contacts.get(app.selected_contact) {
                    if app.backend.remove_contact(&contact.id).is_ok() {
                        app.set_status("Contact removed");
                        app.go_back();
                    }
                }
            }
        }
        _ => {}
    }
}

fn handle_visibility_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(ref contact_id) = app.visibility_state.contact_id {
                if let Ok(fields) = app.backend.get_contact_visibility(contact_id) {
                    if app.visibility_state.selected_field < fields.len().saturating_sub(1) {
                        app.visibility_state.selected_field += 1;
                    }
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.visibility_state.selected_field > 0 {
                app.visibility_state.selected_field -= 1;
            }
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            // Toggle visibility for selected field
            if let Some(ref contact_id) = app.visibility_state.contact_id.clone() {
                if let Ok(fields) = app.backend.get_contact_visibility(contact_id) {
                    if let Some(field) = fields.get(app.visibility_state.selected_field) {
                        match app
                            .backend
                            .toggle_field_visibility(contact_id, &field.field_label)
                        {
                            Ok(now_visible) => {
                                let status = if now_visible {
                                    "now visible"
                                } else {
                                    "now hidden"
                                };
                                app.set_status(format!("Field {} {}", field.field_label, status));
                            }
                            Err(e) => app.set_status(format!("Error: {}", e)),
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn handle_exchange_keys(app: &mut App, key: KeyCode) {
    use crate::ui::exchange::regenerate_qr;
    if let KeyCode::Char('r') = key {
        regenerate_qr(app);
    }
}

fn handle_settings_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('n') | KeyCode::Enter => {
            // Edit display name
            let current_name = app.backend.display_name().unwrap_or("").to_string();
            app.edit_name_state = EditNameState {
                new_name: current_name,
            };
            app.goto(Screen::EditName);
            app.input_mode = InputMode::Editing;
        }
        KeyCode::Char('u') => {
            // Edit relay URL
            let current_url = app.backend.relay_url().to_string();
            app.edit_relay_url_state = EditRelayUrlState {
                new_url: current_url,
            };
            app.goto(Screen::EditRelayUrl);
            app.input_mode = InputMode::Editing;
        }
        KeyCode::Char('b') => app.goto(Screen::Backup),
        KeyCode::Char('d') => app.goto(Screen::Devices),
        KeyCode::Char('r') => app.goto(Screen::Recovery),
        _ => {}
    }
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
                let field_type =
                    Backend::parse_field_type(FIELD_TYPES[app.add_field_state.field_type_index]);
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

fn handle_edit_field_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Enter => {
            // Save the edited field
            let label = app.edit_field_state.field_label.clone();
            let new_value = app.edit_field_state.new_value.trim().to_string();
            if new_value.is_empty() {
                app.set_status("Value cannot be empty");
            } else {
                match app.backend.update_field(&label, &new_value) {
                    Ok(()) => {
                        app.set_status("Field updated");
                        app.go_back();
                    }
                    Err(e) => app.set_status(format!("Error: {}", e)),
                }
            }
        }
        KeyCode::Esc => {
            app.go_back();
        }
        _ => {}
    }
}

fn handle_edit_name_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Enter => {
            // Save the new display name
            let new_name = app.edit_name_state.new_name.trim().to_string();
            if new_name.is_empty() {
                app.set_status("Name cannot be empty");
            } else {
                match app.backend.update_display_name(&new_name) {
                    Ok(()) => {
                        app.set_status("Display name updated");
                        app.go_back();
                    }
                    Err(e) => app.set_status(format!("Error: {}", e)),
                }
            }
        }
        KeyCode::Esc => {
            app.go_back();
        }
        _ => {}
    }
}

fn handle_edit_relay_url_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Enter => {
            // Save the new relay URL
            let new_url = app.edit_relay_url_state.new_url.trim().to_string();
            if new_url.is_empty() {
                app.set_status("URL cannot be empty");
            } else {
                match app.backend.set_relay_url(&new_url) {
                    Ok(()) => {
                        app.set_status("Relay URL updated");
                        app.go_back();
                    }
                    Err(e) => app.set_status(format!("Error: {}", e)),
                }
            }
        }
        KeyCode::Esc => {
            app.go_back();
        }
        _ => {}
    }
}

fn handle_devices_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('j') | KeyCode::Down => {
            if let Ok(devices) = app.backend.list_devices() {
                if app.selected_device < devices.len().saturating_sub(1) {
                    app.selected_device += 1;
                }
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            if app.selected_device > 0 {
                app.selected_device -= 1;
            }
        }
        KeyCode::Char('l') => match app.backend.generate_device_link() {
            Ok(link) => app.set_status(format!("Link code: {}", &link[..40.min(link.len())])),
            Err(e) => app.set_status(format!("Error: {}", e)),
        },
        _ => {}
    }
}

fn handle_recovery_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('c') => {
            app.set_status("Create claim: use CLI 'vauchi recovery claim <old-pk>'");
        }
        KeyCode::Char('v') => {
            app.set_status("Vouch: use CLI 'vauchi recovery vouch <claim>'");
        }
        KeyCode::Char('s') => match app.backend.get_recovery_status() {
            Ok(status) => {
                let msg = format!(
                    "Recovery: {}/{} vouchers",
                    status.voucher_count, status.required_vouchers
                );
                app.set_status(msg);
            }
            Err(e) => app.set_status(format!("Error: {}", e)),
        },
        _ => {}
    }
}

fn handle_sync_keys(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('s') => {
            // Start sync
            if app.sync_state.is_syncing {
                app.set_status("Sync already in progress");
                return;
            }

            // Mark as syncing
            app.sync_state.is_syncing = true;
            app.sync_state.sync_log.push("Starting sync...".to_string());

            // Perform sync
            let result = app.backend.sync();

            // Update state based on result
            app.sync_state.is_syncing = false;

            if result.success {
                app.sync_state.connected = true;
                let summary = format!(
                    "+{} contacts, {} updated, {} sent",
                    result.contacts_added, result.cards_updated, result.updates_sent
                );
                app.sync_state.last_result = Some(summary.clone());
                app.sync_state
                    .sync_log
                    .push(format!("Sync complete: {}", summary));
                app.set_status(format!("Sync complete: {}", summary));

                // Update pending count
                app.sync_state.pending_updates = app.backend.pending_update_count().unwrap_or(0);
            } else {
                app.sync_state.connected = false;
                let error_msg = result.error.unwrap_or_else(|| "Unknown error".to_string());
                app.sync_state.last_result = Some(format!("Failed: {}", error_msg));
                app.sync_state
                    .sync_log
                    .push(format!("Sync failed: {}", error_msg));
                app.set_status(format!("Sync failed: {}", error_msg));
            }
        }
        KeyCode::Char('t') => {
            // Test relay connection
            app.set_status("Testing relay connection...");
            match app.backend.test_relay_connection() {
                Ok(true) => {
                    app.sync_state.connected = true;
                    app.sync_state
                        .sync_log
                        .push("Relay connection test: OK".to_string());
                    app.set_status("Relay connection successful!");
                }
                Ok(false) | Err(_) => {
                    app.sync_state.connected = false;
                    app.sync_state
                        .sync_log
                        .push("Relay connection test: FAILED".to_string());
                    app.set_status("Relay connection failed");
                }
            }
        }
        KeyCode::Char('r') => {
            // Refresh pending update count
            app.sync_state.pending_updates = app.backend.pending_update_count().unwrap_or(0);
            app.set_status(format!(
                "{} pending updates",
                app.sync_state.pending_updates
            ));
        }
        _ => {}
    }
}

fn handle_backup_keys(app: &mut App, key: KeyCode) {
    match app.backup_state.mode {
        BackupMode::Menu => match key {
            KeyCode::Char('e') => {
                app.backup_state.mode = BackupMode::Export;
                app.backup_state.password.clear();
                app.backup_state.confirm_password.clear();
                app.backup_state.focus = BackupFocus::Password;
                app.input_mode = InputMode::Editing;
            }
            KeyCode::Char('i') => {
                app.backup_state.mode = BackupMode::Import;
                app.backup_state.backup_data.clear();
                app.backup_state.password.clear();
                app.backup_state.focus = BackupFocus::Data;
                app.input_mode = InputMode::Editing;
            }
            _ => {}
        },
        BackupMode::Export => match key {
            KeyCode::Tab => {
                app.backup_state.focus = match app.backup_state.focus {
                    BackupFocus::Password => BackupFocus::Confirm,
                    BackupFocus::Confirm => BackupFocus::Password,
                    BackupFocus::Data => BackupFocus::Password,
                };
                app.input_mode = InputMode::Editing;
            }
            KeyCode::Enter => {
                // Check passwords match first
                if app.backup_state.password != app.backup_state.confirm_password {
                    app.set_status("Passwords don't match");
                    return;
                }

                // Validate password strength
                match validate_password(&app.backup_state.password) {
                    Ok(_) => {
                        // Password is strong enough, proceed with export
                        match app.backend.export_backup(&app.backup_state.password) {
                            Ok(data) => {
                                app.set_status(format!(
                                    "Backup: {}...",
                                    &data[..50.min(data.len())]
                                ));
                                app.backup_state.mode = BackupMode::Menu;
                                app.backup_state = Default::default();
                            }
                            Err(e) => app.set_status(format!("Export error: {}", e)),
                        }
                    }
                    Err(_) => {
                        if app.backup_state.password.len() < 8 {
                            app.set_status("Password must be at least 8 characters");
                        } else {
                            app.set_status("Password too weak. Use a stronger passphrase.");
                        }
                    }
                }
            }
            KeyCode::Esc => {
                app.backup_state.mode = BackupMode::Menu;
                app.input_mode = InputMode::Normal;
            }
            _ => {}
        },
        BackupMode::Import => match key {
            KeyCode::Tab => {
                app.backup_state.focus = match app.backup_state.focus {
                    BackupFocus::Data => BackupFocus::Password,
                    BackupFocus::Password => BackupFocus::Data,
                    BackupFocus::Confirm => BackupFocus::Data,
                };
                app.input_mode = InputMode::Editing;
            }
            KeyCode::Enter => {
                if !app.backup_state.backup_data.is_empty() && !app.backup_state.password.is_empty()
                {
                    match app
                        .backend
                        .import_backup(&app.backup_state.backup_data, &app.backup_state.password)
                    {
                        Ok(()) => {
                            app.set_status("Backup imported successfully!");
                            app.backup_state = Default::default();
                            app.go_back();
                        }
                        Err(e) => app.set_status(format!("Import error: {}", e)),
                    }
                } else {
                    app.set_status("Please enter backup data and password");
                }
            }
            KeyCode::Esc => {
                app.backup_state.mode = BackupMode::Menu;
                app.input_mode = InputMode::Normal;
            }
            _ => {}
        },
    }
}
