// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Result routing for `AppEngine` — completion handling, hardware events,
//! and action result dispatch.

use super::AppEngine;
use super::AppScreen;
use crate::contact_card::FieldType;
use crate::exchange::ExchangeHardwareEvent;
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::form_dialog::FormDialogType;

impl AppEngine {
    /// Returns `true` if the current engine has user-entered data that differs
    /// from the original. Used by frontends to show a "discard changes?" prompt.
    pub fn form_has_data(&self) -> bool {
        let dialog_type = match &self.screen {
            AppScreen::FormDialog { dialog_type } => dialog_type,
            _ => return false,
        };
        let input = match self.engine.collected_input() {
            Some(v) => v,
            None => return false,
        };
        match dialog_type {
            FormDialogType::AddField { .. } => {
                // Format: "type\nlabel\nvalue\nnote\ngroups"
                let parts: Vec<&str> = input.splitn(5, '\n').collect();
                if parts.len() >= 3 {
                    let label = parts.get(1).unwrap_or(&"").trim();
                    let value = parts.get(2).unwrap_or(&"").trim();
                    !label.is_empty() || !value.is_empty()
                } else {
                    false
                }
            }
            FormDialogType::EditField { current_value, .. } => input != *current_value,
            FormDialogType::EditName { current_name } => input != *current_name,
            FormDialogType::EditRelayUrl { current_url } => input != *current_url,
        }
    }

    /// Returns all groups as (id, name) pairs for UI forms.
    pub fn available_groups(&self) -> Vec<(String, String)> {
        self.vauchi
            .list_groups()
            .unwrap_or_default()
            .into_iter()
            .map(|g| (g.id().to_string(), g.name().to_string()))
            .collect()
    }

    /// Returns the field type catalog for the Add Field picker.
    pub fn field_type_catalog(&self) -> &crate::contact_card::FieldTypeCatalog {
        &self.field_catalog
    }

    /// Handle a hardware event from the frontend during an exchange (ADR-031).
    ///
    /// Frontends call this when hardware reports results (QR scanned, BLE data
    /// received, etc.). Returns `ExchangeCommands` with response commands, or
    /// a screen update if the exchange state changed (e.g., verification started).
    ///
    /// Returns `None` if the current screen is not an exchange screen.
    pub fn handle_hardware_event(&mut self, event: ExchangeHardwareEvent) -> Option<ActionResult> {
        if !matches!(self.screen, AppScreen::Exchange) {
            return None;
        }

        // ADR-031: For error events, build a user-friendly UI response
        // before delegating to the engine (which may transition to Failed).
        let ui_override = match &event {
            ExchangeHardwareEvent::HardwareUnavailable { transport } => {
                Some(ActionResult::ShowToast {
                    message: format!("{} is not available on this device", transport),
                    undo_action_id: None,
                })
            }
            ExchangeHardwareEvent::HardwareError { transport, error } => {
                Some(ActionResult::ShowAlert {
                    title: format!("{} error", transport),
                    message: error.clone(),
                })
            }
            _ => None,
        };

        // Delegate to the engine via the WorkflowEngine trait (ADR-031).
        // ExchangeEngine handles session-aware events; other engines return None.
        if let Some(result) = self.engine.handle_hardware_event(event) {
            // Prefer user-friendly error messages over raw session results
            return Some(ui_override.unwrap_or(result));
        }

        // Engine didn't handle it — return error UI if applicable
        if let Some(ui) = ui_override {
            return Some(ui);
        }

        None
    }

    pub(super) fn handle_completion(&mut self) -> ActionResult {
        match &self.screen {
            AppScreen::Onboarding => {
                let name = match self.pending_display_name.take() {
                    Some(n) if !n.trim().is_empty() => n,
                    _ => {
                        return ActionResult::ValidationError {
                            component_id: "display_name".into(),
                            message: "Please enter a display name".into(),
                        };
                    }
                };
                match self.vauchi.create_identity(&name) {
                    Ok(()) => {
                        let screen = self.navigate_to_internal(AppScreen::MyInfo);
                        ActionResult::NavigateTo(screen)
                    }
                    Err(e) => ActionResult::ShowAlert {
                        title: "Error".into(),
                        message: format!("Failed to create identity: {e}"),
                    },
                }
            }
            AppScreen::Lock => {
                let pin = match self.engine.collected_input() {
                    Some(p) => p,
                    None => {
                        return ActionResult::ValidationError {
                            component_id: "pin".into(),
                            message: "Please enter your password".into(),
                        };
                    }
                };
                match self.vauchi.authenticate(&pin) {
                    Ok(_auth_mode) => {
                        let screen = self.navigate_to_internal(AppScreen::MyInfo);
                        ActionResult::NavigateTo(screen)
                    }
                    Err(_) => {
                        // Notify lock engine of failed auth so it tracks attempts
                        // and clears the entered PIN.
                        self.engine.handle_action(UserAction::ActionPressed {
                            action_id: "auth_failed".into(),
                        })
                    }
                }
            }
            AppScreen::Exchange => {
                let screen = self.navigate_to_internal(AppScreen::Contacts);
                ActionResult::NavigateTo(screen)
            }
            AppScreen::EmergencyShred => {
                let screen = self.navigate_to_internal(AppScreen::Onboarding);
                ActionResult::NavigateTo(screen)
            }
            AppScreen::FormDialog { ref dialog_type } => {
                // Cancel navigates back without saving
                if self.engine.was_cancelled() {
                    let screen = self.navigate_back();
                    return ActionResult::NavigateTo(screen);
                }
                let input = self.engine.collected_input();
                let result = match dialog_type {
                    FormDialogType::EditName { .. } => {
                        let name = input.unwrap_or_default();
                        if name.trim().is_empty() {
                            return ActionResult::ValidationError {
                                component_id: "display_name".into(),
                                message: "Display name cannot be empty".into(),
                            };
                        }
                        self.vauchi.update_display_name(&name)
                    }
                    FormDialogType::EditField { field_id, .. } => {
                        let value = input.unwrap_or_default();
                        match self.vauchi.own_card() {
                            Ok(Some(mut card)) => {
                                if let Err(e) = card.update_field_value(field_id, &value) {
                                    return ActionResult::ShowAlert {
                                        title: "Error".into(),
                                        message: format!("Failed to update field: {e}"),
                                    };
                                }
                                self.vauchi.update_own_card(&card).map(|_| ())
                            }
                            Ok(None) => {
                                return ActionResult::ShowAlert {
                                    title: "Error".into(),
                                    message: "No contact card found".into(),
                                };
                            }
                            Err(e) => Err(e),
                        }
                    }
                    FormDialogType::AddField { .. } => {
                        let raw = input.unwrap_or_default();
                        // Format: type\nlabel\nvalue\nnote\ngroups
                        let mut lines = raw.splitn(5, '\n');
                        let entry_type = lines.next().unwrap_or("custom").trim();
                        let label_input = lines.next().unwrap_or("").trim();
                        let value = lines.next().unwrap_or("").trim();
                        let _note = lines.next().unwrap_or("").trim();
                        let _groups = lines.next().unwrap_or("").trim();
                        if value.is_empty() {
                            return ActionResult::ValidationError {
                                component_id: "field_value".into(),
                                message: "Value cannot be empty".into(),
                            };
                        }
                        let field_type = match entry_type {
                            "phone" => FieldType::Phone,
                            "email" => FieldType::Email,
                            "social" => FieldType::Social,
                            "address" => FieldType::Address,
                            "website" => FieldType::Website,
                            "birthday" => FieldType::Birthday,
                            _ => FieldType::Custom,
                        };
                        // Use label_input as label if provided, otherwise use type name
                        let label = if label_input.is_empty() {
                            entry_type
                                .chars()
                                .next()
                                .map(|c| c.to_uppercase().to_string() + &entry_type[1..])
                                .unwrap_or_else(|| "Custom".into())
                        } else {
                            label_input.to_string()
                        };
                        let field =
                            crate::contact_card::ContactField::new(field_type, &label, value);
                        let field_id = field.id().to_string();
                        let result = self.vauchi.add_own_field(field);
                        // Apply group visibility from selected groups
                        if result.is_ok() && !_groups.is_empty() {
                            for group_id in _groups.split(',').map(|s| s.trim()) {
                                if !group_id.is_empty() {
                                    let _ = self
                                        .vauchi
                                        .set_group_field_visibility(group_id, &field_id, true);
                                }
                            }
                        }
                        result
                    }
                    FormDialogType::EditRelayUrl { .. } => {
                        // Relay URL is TUI-specific config (Backend), not in Vauchi.
                        // Navigate back; TUI handles save via Backend::set_relay_url.
                        Ok(())
                    }
                };
                match result {
                    Ok(()) => {
                        // Invalidate parent screen cache so it refreshes with updated data
                        if let Some(parent) = self.nav_history.last() {
                            self.engine_cache.remove(parent);
                        }
                        let screen = self.navigate_back();
                        ActionResult::NavigateTo(screen)
                    }
                    Err(e) => ActionResult::ShowAlert {
                        title: "Error".into(),
                        message: format!("{e}"),
                    },
                }
            }
            _ => {
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
        }
    }

    /// Route engine results to appropriate navigation targets.
    pub(super) fn route_result(&mut self, result: ActionResult) -> ActionResult {
        match result {
            ActionResult::Complete => self.handle_completion(),
            ActionResult::EditContact { contact_id } => {
                let screen = self.navigate_to(AppScreen::ContactEdit { contact_id });
                ActionResult::NavigateTo(screen)
            }
            ActionResult::OpenEntryDetail { field_id } => {
                let screen = self.navigate_to(AppScreen::MyInfoEntryDetail { field_id });
                ActionResult::NavigateTo(screen)
            }
            // GroupsEngine reuses OpenContact to signal group selection.
            // Route to GroupDetail when on the Groups screen.
            ActionResult::OpenContact { contact_id } if self.screen == AppScreen::Groups => {
                let screen = self.navigate_to(AppScreen::GroupDetail {
                    group_id: contact_id,
                });
                ActionResult::NavigateTo(screen)
            }
            other => other,
        }
    }
}
