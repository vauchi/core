// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Result routing for `AppEngine` — completion handling, hardware events,
//! and action result dispatch.

use super::AppEngine;
use super::AppScreen;
use crate::ui::ScreenModel;
use crate::ui::action::{ActionResult, ContactActionKind, PostOnboardingDestination, UserAction};
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
            FormDialogType::CreateGroup => !input.is_empty(),
            FormDialogType::RenameGroup { current_name, .. } => input != *current_name,
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

    /// Handle a hardware event from the frontend (ADR-031).
    ///
    /// Frontends call this when hardware reports results (QR scanned, BLE data
    /// received, image picked, etc.). Returns `ExchangeCommands` with response
    /// commands, or a screen update if the engine state changed.
    ///
    /// Returns `None` if the current screen doesn't handle hardware events.
    #[tracing::instrument(level = "debug", skip_all, name = "app.handle_hardware_event")]
    pub fn handle_hardware_event(&mut self, event: ExchangeHardwareEvent) -> Option<ActionResult> {
        if !matches!(
            self.screen,
            AppScreen::Exchange
                | AppScreen::AvatarEditor
                | AppScreen::Recovery
                | AppScreen::MultiStageExchange
        ) {
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
            ExchangeHardwareEvent::PermissionDenied { transport } => {
                Some(ActionResult::ShowToast {
                    message: format!("{} access was denied", transport),
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
            // Navigation and command results take priority over informational
            // toasts — the engine handled the event with a state transition
            // (e.g., camera denied → ManualEntry). Toasts are only used when
            // the engine returns a simple screen update.
            if matches!(
                result,
                ActionResult::NavigateTo(_) | ActionResult::ExchangeCommands { .. }
            ) {
                return Some(result);
            }
            return Some(ui_override.unwrap_or(result));
        }

        // Engine didn't handle it — return error UI if applicable
        if let Some(ui) = ui_override {
            return Some(ui);
        }

        None
    }

    /// Advance the animated QR to its next frame (~10fps timer from the frontend).
    ///
    /// Delegates to the active engine's `WorkflowEngine::advance_qr_frame`. Only
    /// `ExchangeEngine` on the ShowQr step responds — everything else returns
    /// `None`, so frontends can safely tick the timer without guarding on screen.
    pub fn advance_qr_frame(&mut self) -> Option<ScreenModel> {
        if !matches!(self.screen, AppScreen::Exchange) {
            return None;
        }
        self.engine.advance_qr_frame()
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
                match self.vauchi.create_identity(&name) {
                    Ok(()) => {
                        // Persist onboarding groups
                        for group_name in &onboarding_groups {
                            let _ = self.vauchi.create_group(group_name);
                        }
                        let target = AppScreen::MyInfo;
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
                // ADR-031: Extract exchange result BEFORE navigate_to_internal
                // replaces the engine (navigation.rs:34 does std::mem::replace).
                let exchange_data = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::exchange::ExchangeEngine>())
                    .and_then(|ex| {
                        let groups = ex.selected_groups().to_vec();
                        // QR path: contact is in session.state() → Complete { contact }
                        if let Some(session) = ex.session()
                            && let vauchi_core::exchange::ExchangeState::Complete { contact } =
                                session.state()
                        {
                            return Some((*contact.clone(), groups));
                        }
                        None
                    });

                let screen = self.navigate_to_internal(AppScreen::Contacts);

                // Persist exchange result: upsert contact + init ratchet + assign groups
                if let Some((contact, groups)) = exchange_data {
                    let contact_id = contact.id().to_string();
                    if let Err(e) = self.vauchi.update_contact(&contact) {
                        return ActionResult::ShowAlert {
                            title: "Exchange Error".into(),
                            message: format!("Failed to save contact: {e}"),
                        };
                    }
                    if let (Some(sk), Some(pk)) = (contact.shared_key(), contact.public_key())
                        && let Err(e) =
                            self.vauchi
                                .create_ratchet_as_initiator(&contact_id, sk, *pk)
                    {
                        return ActionResult::ShowAlert {
                            title: "Exchange Error".into(),
                            message: format!("Failed to initialize encryption: {e}"),
                        };
                    }
                    for group_id in &groups {
                        let _ = self.vauchi.add_contact_to_group(group_id, &contact_id);
                    }
                }

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
                use crate::ui::fingerprint_verify::VerifyAction;
                let fp_engine = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::FingerprintVerifyEngine>());
                if let Some(fp_engine) = fp_engine {
                    match fp_engine.completion_action() {
                        VerifyAction::Verified => {
                            let _ = self.vauchi.verify_contact_fingerprint(contact_id);
                        }
                        VerifyAction::Unverified => {
                            let _ = self.vauchi.unverify_contact_fingerprint(contact_id);
                        }
                        VerifyAction::None => {}
                    }
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
                    FormDialogType::CreateGroup => {
                        let name = input.unwrap_or_default();
                        if name.trim().is_empty() {
                            return ActionResult::ValidationError {
                                component_id: "group_name".into(),
                                message: "Group name cannot be empty".into(),
                            };
                        }
                        self.vauchi.create_group(name.trim()).map(|_| ())
                    }
                    FormDialogType::RenameGroup { group_id, .. } => {
                        let name = input.unwrap_or_default();
                        if name.trim().is_empty() {
                            return ActionResult::ValidationError {
                                component_id: "group_name".into(),
                                message: "Group name cannot be empty".into(),
                            };
                        }
                        self.vauchi.rename_group(group_id, name.trim())
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
            AppScreen::Sync => {
                let action = self.engine.collected_input().unwrap_or_default();
                match action.as_str() {
                    "sync_now" => {
                        let pending = self.vauchi.pending_update_count().unwrap_or(0);
                        if pending == 0 {
                            ActionResult::ShowToast {
                                message: "Already up to date".into(),
                                undo_action_id: None,
                            }
                        } else {
                            ActionResult::ShowToast {
                                message: format!("{pending} update(s) queued for sync"),
                                undo_action_id: None,
                            }
                        }
                    }
                    "test_connection" => ActionResult::ShowToast {
                        message: "Connection check initiated".into(),
                        undo_action_id: None,
                    },
                    _ => {
                        let screen = self.navigate_back();
                        ActionResult::NavigateTo(screen)
                    }
                }
            }
            AppScreen::DuressPin => {
                let dp_engine = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::DuressPinEngine>());
                if let Some(dp_engine) = dp_engine {
                    let config = dp_engine.config();
                    if config.enabled {
                        let pin = dp_engine.pin();
                        if let Err(e) = self.vauchi.setup_duress_password(pin) {
                            return ActionResult::ShowAlert {
                                title: "Error".into(),
                                message: format!("Failed to set duress PIN: {e}"),
                            };
                        }
                        let settings = vauchi_core::types::DuressSettings {
                            alert_contact_ids: config
                                .alert_contacts
                                .iter()
                                .map(|c| c.id.clone())
                                .collect(),
                            alert_message: config.alert_message.clone(),
                            include_location: config.include_location,
                        };
                        if let Err(e) = self.vauchi.save_duress_settings(&settings) {
                            return ActionResult::ShowAlert {
                                title: "Error".into(),
                                message: format!("Failed to save duress settings: {e}"),
                            };
                        }
                    } else if let Err(e) = self.vauchi.disable_duress() {
                        return ActionResult::ShowAlert {
                            title: "Error".into(),
                            message: format!("Failed to disable duress: {e}"),
                        };
                    }
                }
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::DeviceManagement => {
                // Read the confirmed index from the engine before navigating away
                let revoke_index = self
                    .engine
                    .as_any()
                    .and_then(|a| {
                        a.downcast_ref::<crate::ui::device_management::DeviceManagementEngine>()
                    })
                    .and_then(|e| e.confirmed_revoke_index());

                if let Some(idx) = revoke_index {
                    match self.vauchi.revoke_device(idx as usize) {
                        Ok(_name) => {
                            // Refresh the device list after revocation
                            let screen = self.navigate_to_internal(AppScreen::DeviceManagement);
                            return ActionResult::NavigateTo(screen);
                        }
                        Err(e) => {
                            return ActionResult::ShowAlert {
                                title: "Revoke Failed".into(),
                                message: format!("{e}"),
                            };
                        }
                    }
                }
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::ContactDetail { contact_id } => {
                // InlineConfirm → hard delete the imported contact and navigate back.
                let contact_id = contact_id.clone();
                let _ = self.vauchi.hard_delete_imported_contact(&contact_id);
                self.engine_cache.remove(&AppScreen::Contacts);
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::GroupDetail { group_id } => {
                let group_id = group_id.clone();
                let _ = self.vauchi.delete_group(&group_id);
                self.engine_cache.remove(&AppScreen::Groups);
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::Groups => {
                if let Some(group_id) = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::groups_list::GroupsEngine>())
                    .and_then(|e| e.pending_delete_group_id().map(|s| s.to_string()))
                {
                    let _ = self.vauchi.delete_group(&group_id);
                }
                self.engine_cache.remove(&AppScreen::Groups);
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::ContactMerge { .. } => {
                match self.pending_merge.take() {
                    Some((primary_id, secondary_id)) => {
                        match self.vauchi.merge_contacts(&primary_id, &secondary_id) {
                            Ok(_merged) => {
                                self.engine_cache.remove(&AppScreen::Contacts);
                                self.engine_cache.remove(&AppScreen::ContactDuplicates);
                                // Navigate back to ContactDuplicates (or wherever we came from).
                                // navigate_back() mutates nav state; ShowToast causes the
                                // frontend to re-query current_screen() for the updated view.
                                self.navigate_back();
                                ActionResult::ShowToast {
                                    message: "Contacts merged".into(),
                                    undo_action_id: None,
                                }
                            }
                            Err(e) => ActionResult::ShowAlert {
                                title: "Merge Failed".into(),
                                message: format!("{e}"),
                            },
                        }
                    }
                    None => {
                        // No pending merge state — just navigate back
                        let screen = self.navigate_back();
                        ActionResult::NavigateTo(screen)
                    }
                }
            }
            AppScreen::AvatarEditor => {
                if self.engine.was_cancelled() {
                    let screen = self.navigate_back();
                    return ActionResult::NavigateTo(screen);
                }
                let editor = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::avatar_editor::AvatarEditorEngine>());
                if let Some(editor) = editor {
                    if editor.avatar_removed() {
                        // Clear avatar from own card
                        if let Ok(Some(mut card)) = self.vauchi.own_card() {
                            card.clear_avatar();
                            let _ = self.vauchi.update_own_card(&card);
                        }
                    } else if let Some(avatar) = editor.result_avatar() {
                        // Persist the new avatar
                        if let Ok(Some(mut card)) = self.vauchi.own_card() {
                            let _ = card.set_avatar(avatar.to_vec());
                            let _ = self.vauchi.update_own_card(&card);
                        }
                    }
                }
                self.invalidate_screen(&AppScreen::MyInfo);
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::DeviceReplacement => {
                if self.engine.was_cancelled() {
                    let screen = self.navigate_back();
                    return ActionResult::NavigateTo(screen);
                }
                // Check if user chose to decommission old device
                let outcome = self
                    .engine
                    .as_any()
                    .and_then(|a| {
                        a.downcast_ref::<crate::ui::device_replacement::DeviceReplacementEngine>()
                    })
                    .map(|e| e.completion_outcome().clone());
                if let Some(crate::ui::device_replacement::CompletionOutcome::RemoveOldDevice) =
                    outcome
                {
                    // Delegate to existing device management unlink
                    // (current device index = 0, handled by the platform layer)
                    self.navigate_back();
                    return ActionResult::ShowToast {
                        message: "Device removal requested. Complete in Settings > Devices.".into(),
                        undo_action_id: None,
                    };
                }
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
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
            ActionResult::ContactAction { contact_id, kind } => {
                self.apply_contact_action(&contact_id, kind)
            }
            ActionResult::Complete => self.handle_completion(),
            ActionResult::CompleteWith { destination } => {
                let base_result = self.handle_completion();
                if matches!(
                    base_result,
                    ActionResult::ValidationError { .. } | ActionResult::ShowAlert { .. }
                ) {
                    return base_result;
                }
                let target = match destination {
                    PostOnboardingDestination::MainScreen => AppScreen::MyInfo,
                    PostOnboardingDestination::Exchange => AppScreen::Exchange,
                    PostOnboardingDestination::ImportContacts => AppScreen::Contacts,
                    PostOnboardingDestination::SecurityInfo => AppScreen::Help,
                    PostOnboardingDestination::BackupSetup => AppScreen::Backup,
                };
                let screen = self.navigate_to(target);
                ActionResult::NavigateTo(screen)
            }
            ActionResult::EditContact { contact_id } => {
                let screen = self.navigate_to(AppScreen::ContactEdit { contact_id });
                ActionResult::NavigateTo(screen)
            }
            // DeviceManagementEngine emits StartDeviceLink when the user
            // taps "Link New Device". The link flow is fully core-driven
            // (`DeviceLinkingEngine` shows the QR + handles verify code +
            // sync), so route the user straight there. Onboarding and
            // DeviceReplacement also emit StartDeviceLink, but those
            // happen on screens with their own existing native flows
            // — we leave their results untouched so frontends keep
            // calling `viewModel.startDeviceLinkInitiator()` /
            // equivalent during onboarding.
            ActionResult::StartDeviceLink if self.screen == AppScreen::DeviceManagement => {
                let screen = self.navigate_to(AppScreen::DeviceLinking);
                ActionResult::NavigateTo(screen)
            }
            // Pair 4 — `ExchangeEngine` emits StartMultiStageExchange when
            // the user picks `ExchangeMode::Glance` from the mode list.
            // Route to the dedicated screen so `PlatformAppEngine` can
            // auto-create the `MobileMultiStageSession` on entry. The
            // frontend never decides this — picking a mode is a
            // user-action, the rest is core's responsibility.
            ActionResult::StartMultiStageExchange => {
                let screen = self.navigate_to(AppScreen::MultiStageExchange);
                ActionResult::NavigateTo(screen)
            }
            ActionResult::OpenEntryDetail { field_id } => {
                let screen = self.navigate_to(AppScreen::MyInfoEntryDetail { field_id });
                ActionResult::NavigateTo(screen)
            }
            ActionResult::VerifyFingerprint { contact_id } => {
                let screen = self.navigate_to(AppScreen::VerifyFingerprint { contact_id });
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
            // Group management: route ShowFormDialog to FormDialog screen
            ActionResult::ShowFormDialog {
                dialog_type,
                context_id,
            } => {
                let form_type = match dialog_type.as_str() {
                    "create_group" => Some(crate::ui::form_dialog::FormDialogType::CreateGroup),
                    "rename_group" => {
                        let group_id = context_id.unwrap_or_default();
                        let current_name = self
                            .vauchi
                            .get_group(&group_id)
                            .ok()
                            .map(|g| g.name().to_string())
                            .unwrap_or_default();
                        Some(crate::ui::form_dialog::FormDialogType::RenameGroup {
                            group_id,
                            current_name,
                        })
                    }
                    _ => None,
                };
                if let Some(ft) = form_type {
                    let screen = self.navigate_to(AppScreen::FormDialog { dialog_type: ft });
                    ActionResult::NavigateTo(screen)
                } else {
                    ActionResult::UpdateScreen(self.engine.current_screen())
                }
            }
            // Intercept backup Processing: execute backup in core, return result screen
            ActionResult::NavigateTo(ref screen)
                if self.screen == AppScreen::Backup && screen.screen_id == "backup_processing" =>
            {
                self.execute_backup()
            }
            // Persist field-visibility toggles emitted by GroupDetailEngine
            // (Pair 2 of Pure Humble UI retirement). Calls the
            // repropagating variant so downstream contacts re-fetch the
            // visible field set on the next sync. Engine cache is
            // invalidated so the Visible Fields count refreshes.
            ActionResult::SetGroupFieldVisibility {
                group_id,
                field_id,
                visible,
            } => {
                let _ = self
                    .vauchi
                    .set_group_field_visibility_and_repropagate(&group_id, &field_id, visible);
                self.engine_cache.remove(&self.screen);
                ActionResult::UpdateScreen(self.engine.current_screen())
            }
            // Reschedule every failed delivery for immediate retry —
            // mirror of `mobile_delivery::manual_retry`, applied per id.
            // Emitted by DeliveryStatusEngine on the "Retry Failed"
            // footer (Pair 1 of Pure Humble UI retirement).
            ActionResult::RetryFailedDeliveries { message_ids } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let mut rescheduled = 0u32;
                for id in &message_ids {
                    if let Ok(Some(_)) = self.vauchi.storage().get_retry_entry(id)
                        && self
                            .vauchi
                            .storage()
                            .update_retry_next_time(id, now)
                            .is_ok()
                    {
                        rescheduled += 1;
                    }
                }
                self.engine_cache.remove(&self.screen);
                ActionResult::ShowToast {
                    message: if rescheduled == 1 {
                        "Retry scheduled for 1 message".to_string()
                    } else {
                        format!("Retry scheduled for {rescheduled} messages")
                    },
                    undo_action_id: None,
                }
            }
            other => other,
        }
    }

    /// Execute backup export using captured password and level.
    ///
    /// Called when the BackupRecoveryEngine transitions to Processing.
    /// Runs the backup operation synchronously (Argon2id KDF is slow but
    /// the platform already calls handle_action on a background thread).
    ///
    /// Import (restore) is not handled here — it needs the raw backup file
    /// from a platform file picker, so restore uses `StartBackupImport`.
    fn execute_backup(&mut self) -> ActionResult {
        use crate::ui::backup_recovery::BackupRecoveryEngine;

        // Read mode from the engine via downcast (avoids fragile string matching)
        let is_restore = self
            .engine
            .as_any()
            .and_then(|a| a.downcast_ref::<BackupRecoveryEngine>())
            .is_some_and(|e| *e.mode() == crate::ui::backup_recovery::BackupMode::Restore);

        if is_restore {
            // Restore needs backup data from a file picker — not available here.
            self.engine.processing_failed();
            self.pending_backup_full = true;
            return ActionResult::NavigateTo(self.engine.current_screen());
        }

        let password = match self.pending_backup_password.take() {
            Some(p) => p,
            None => {
                self.engine.processing_failed();
                self.pending_backup_full = true;
                return ActionResult::NavigateTo(self.engine.current_screen());
            }
        };

        let result = if self.pending_backup_full {
            self.vauchi.export_full_backup(&password)
        } else {
            self.vauchi.export_backup(&password)
        };

        // Reset captured state
        self.pending_backup_full = true;

        match result {
            Ok(data) => {
                self.engine.processing_complete();
                ActionResult::BackupExportComplete { data }
            }
            Err(_) => {
                self.engine.processing_failed();
                ActionResult::NavigateTo(self.engine.current_screen())
            }
        }
    }

    /// Apply a per-row contact mutation triggered by `ContactListEngine`.
    ///
    /// Calls the matching `Vauchi` op, invalidates the contact-list cache so
    /// the next screen read reflects the new state, then emits a
    /// `ShowToast` carrying an `undo_action_id` for reversible mutations.
    /// The undo id is a `undo_<verb>_contact:<contact_id>` string that
    /// `AppEngine::handle_undo` already matches on — see
    /// `intercept.rs::handle_undo`.
    pub(super) fn apply_contact_action(
        &mut self,
        contact_id: &str,
        kind: ContactActionKind,
    ) -> ActionResult {
        // Invalidate the list cache regardless of outcome so a failed op
        // still yields a fresh screen read.
        self.engine_cache.remove(&AppScreen::Contacts);
        self.engine_cache.remove(&AppScreen::ArchivedContacts);
        match kind {
            ContactActionKind::Archive => {
                let _ = self.vauchi.archive_contact(contact_id);
                self.pending_contact_undo = Some(super::PendingContactUndo::Archive {
                    contact_id: contact_id.to_string(),
                });
                ActionResult::ShowToast {
                    message: "Contact archived".into(),
                    undo_action_id: Some(format!("undo_archive_contact:{contact_id}")),
                }
            }
            ContactActionKind::Unarchive => {
                let _ = self.vauchi.unarchive_contact(contact_id);
                ActionResult::ShowToast {
                    message: "Contact unarchived".into(),
                    undo_action_id: None,
                }
            }
            ContactActionKind::Hide => {
                let _ = self.vauchi.hide_contact(contact_id);
                ActionResult::ShowToast {
                    message: "Contact hidden".into(),
                    undo_action_id: Some(format!("undo_hide_contact:{contact_id}")),
                }
            }
            ContactActionKind::Unhide => {
                let _ = self.vauchi.unhide_contact(contact_id);
                ActionResult::ShowToast {
                    message: "Contact unhidden".into(),
                    undo_action_id: None,
                }
            }
            ContactActionKind::Delete => {
                let _ = self.vauchi.soft_delete_imported_contact(contact_id);
                ActionResult::ShowToast {
                    message: "Contact deleted".into(),
                    undo_action_id: Some(format!("undo_delete_contact:{contact_id}")),
                }
            }
            ContactActionKind::Undelete => {
                let _ = self.vauchi.undo_delete_imported_contact(contact_id);
                ActionResult::ShowToast {
                    message: "Contact restored".into(),
                    undo_action_id: None,
                }
            }
        }
    }
}
