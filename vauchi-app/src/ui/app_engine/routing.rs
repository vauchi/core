// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Result routing for `AppEngine` — completion handling, hardware events,
//! and action result dispatch.

use super::AppEngine;
use super::AppScreen;
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::form_dialog::FormDialogType;
use vauchi_core::contact_card::FieldType;
use vauchi_core::exchange::ExchangeHardwareEvent;

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
            FormDialogType::EditField {
                current_value,
                current_note,
                ..
            } => {
                // Format: "value\nnote"
                let mut parts = input.splitn(2, '\n');
                let value = parts.next().unwrap_or("");
                let note = parts.next().unwrap_or("");
                value != current_value.as_str() || note != current_note.as_deref().unwrap_or("")
            }
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
    pub fn field_type_catalog(&self) -> &vauchi_core::contact_card::FieldTypeCatalog {
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
                // Extract data from the active onboarding engine before identity
                // creation (engine will be discarded after navigating away).
                let onboarding_engine = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::onboarding::OnboardingEngine>());
                let onboarding_groups: Vec<String> = onboarding_engine
                    .map(|ob| {
                        ob.onboarding_data()
                            .selected_groups
                            .iter()
                            .filter(|g| g.selected)
                            .map(|g| g.name.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                let backup_requested = onboarding_engine.is_some_and(|ob| ob.backup_requested());

                match self.vauchi.create_identity(&name) {
                    Ok(()) => {
                        // Persist onboarding groups
                        for group_name in &onboarding_groups {
                            let _ = self.vauchi.create_group(group_name);
                        }
                        let target = if backup_requested {
                            AppScreen::Backup
                        } else {
                            AppScreen::MyInfo
                        };
                        let screen = self.navigate_to_internal(target);
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
            AppScreen::ContactVisibility { contact_id } => {
                if let Some(input) = self.engine.collected_input() {
                    // Parse "field_id:visible,field_id:hidden,..." and persist
                    let contact_id = contact_id.clone();
                    for pair in input.split(',') {
                        let mut parts = pair.splitn(2, ':');
                        if let (Some(field_id), Some(state)) = (parts.next(), parts.next()) {
                            let should_show = state == "visible";
                            let is_visible = self
                                .vauchi
                                .get_effective_field_visibility(&contact_id, field_id)
                                .unwrap_or(true);
                            if should_show != is_visible {
                                let _ = self.vauchi.toggle_field_visibility(&contact_id, field_id);
                            }
                        }
                    }
                }
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::VerifyFingerprint { contact_id } => {
                if !self.engine.was_cancelled() {
                    let _ = self.vauchi.verify_contact_fingerprint(contact_id);
                }
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::EmergencyShred => {
                let screen = self.navigate_to_internal(AppScreen::Onboarding);
                ActionResult::NavigateTo(screen)
            }
            AppScreen::Privacy => {
                // GdprEngine returns "export" or "delete" via collected_input().
                // The actual API calls happen in the platform layer (UniFFI/CABI);
                // here we just navigate back and show feedback.
                let action = self.engine.collected_input().unwrap_or_default();
                match action.as_str() {
                    "export" => ActionResult::ShowToast {
                        message: "Data export requested. Check your files.".into(),
                        undo_action_id: None,
                    },
                    "delete" => ActionResult::ShowToast {
                        message: "Identity deletion scheduled. You have 7 days to cancel.".into(),
                        undo_action_id: None,
                    },
                    _ => {
                        let screen = self.navigate_back();
                        ActionResult::NavigateTo(screen)
                    }
                }
            }
            AppScreen::FormDialog { dialog_type } => {
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
                        let raw = input.unwrap_or_default();
                        // Format: value\nnote
                        let mut parts = raw.splitn(2, '\n');
                        let value = parts.next().unwrap_or("").to_string();
                        let note = parts.next().unwrap_or("").trim().to_string();
                        match self.vauchi.own_card() {
                            Ok(Some(mut card)) => {
                                if let Err(e) = card.update_field_value(field_id, &value) {
                                    return ActionResult::ShowAlert {
                                        title: "Error".into(),
                                        message: format!("Failed to update field: {e}"),
                                    };
                                }
                                let note_opt = if note.is_empty() { None } else { Some(note) };
                                if let Err(e) = card.update_field_note(field_id, note_opt) {
                                    return ActionResult::ShowAlert {
                                        title: "Error".into(),
                                        message: format!("Failed to update field note: {e}"),
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
                        let note = lines.next().unwrap_or("").trim();
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
                            s if s.starts_with("social:") => FieldType::Social,
                            "address" => FieldType::Address,
                            "website" => FieldType::Website,
                            "birthday" => FieldType::Birthday,
                            _ => FieldType::Custom,
                        };
                        // Use label_input if provided, otherwise derive from catalog
                        let label = if !label_input.is_empty() {
                            label_input.to_string()
                        } else if let Some(entry) = self.field_catalog.get(entry_type) {
                            entry.display_name.clone()
                        } else {
                            entry_type
                                .chars()
                                .next()
                                .map(|c| c.to_uppercase().to_string() + &entry_type[1..])
                                .unwrap_or_else(|| "Custom".into())
                        };
                        let mut field =
                            vauchi_core::contact_card::ContactField::new(field_type, &label, value);
                        if !note.is_empty() {
                            field = field.with_note(note.to_string());
                        }
                        let field_id = field.id().to_string();
                        let group_list: Vec<String> = _groups
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        let result = self.vauchi.add_own_field(field);
                        // Apply group visibility from selected groups
                        if result.is_ok() && !group_list.is_empty() {
                            for group_id in &group_list {
                                let _ = self
                                    .vauchi
                                    .set_group_field_visibility(group_id, &field_id, true);
                            }
                        }
                        // During onboarding, also buffer the field in the cached
                        // OnboardingEngine so build_contact_info shows it.
                        if result.is_ok()
                            && let Some(parent) = self.nav_history.last()
                            && matches!(parent, AppScreen::Onboarding)
                            && let Some(engine) = self.engine_cache.get_mut(parent)
                            && let Some(ob) = engine.as_any_mut().and_then(|a| {
                                a.downcast_mut::<crate::ui::onboarding::OnboardingEngine>()
                            })
                        {
                            ob.push_field(crate::ui::onboarding::FieldSetup {
                                field_type: entry_type.to_string(),
                                label: label.clone(),
                                value: value.to_string(),
                                visible_to_groups: group_list,
                                shown: true,
                            });
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
                        // Invalidate parent screen cache so it refreshes with updated data.
                        // Exception: don't invalidate Onboarding — its state machine
                        // (step, groups, name) must survive FormDialog round-trips.
                        // The onboarding ContactInfo screen syncs fields from storage.
                        if let Some(parent) = self.nav_history.last()
                            && !matches!(parent, AppScreen::Onboarding)
                        {
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
            // ContactDetailEngine uses "verify:{id}" to navigate to fingerprint verification.
            ActionResult::OpenContact { ref contact_id } if contact_id.starts_with("verify:") => {
                let real_id = contact_id.strip_prefix("verify:").unwrap().to_string();
                let screen = self.navigate_to(AppScreen::VerifyFingerprint {
                    contact_id: real_id,
                });
                ActionResult::NavigateTo(screen)
            }
            // MoreEngine reuses OpenContact to signal menu selection.
            // Route to the target screen when on the More screen.
            ActionResult::OpenContact { contact_id } if self.screen == AppScreen::More => {
                match AppScreen::from_screen_id(&contact_id) {
                    Some(target) => {
                        let screen = self.navigate_to(target);
                        ActionResult::NavigateTo(screen)
                    }
                    None => ActionResult::UpdateScreen(self.engine.current_screen()),
                }
            }
            // GroupsEngine reuses OpenContact to signal group selection.
            // Route to GroupDetail when on the Groups screen.
            ActionResult::OpenContact { contact_id } if self.screen == AppScreen::Groups => {
                let screen = self.navigate_to(AppScreen::GroupDetail {
                    group_id: contact_id,
                });
                ActionResult::NavigateTo(screen)
            }
            // Navigate to MyInfo in preview mode for the given contact.
            ActionResult::PreviewAs { contact_id } => {
                let screen = self.preview_as(contact_id);
                ActionResult::NavigateTo(screen)
            }
            // Navigate to Contacts screen so the user can pick a contact to preview as.
            ActionResult::ShowContactPicker => {
                let screen = self.navigate_to(AppScreen::Contacts);
                ActionResult::NavigateTo(screen)
            }
            other => other,
        }
    }
}
