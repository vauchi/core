// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Action interception methods for `AppEngine` — settings routing, entry detail
//! actions, add-field dialog, and undo support.

use super::AppEngine;
use super::AppScreen;
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::contact_detail::ContactDetailEngine;
use crate::ui::engine::WorkflowEngine;
use crate::ui::form_dialog::FormDialogType;
use crate::ui::my_info_entry_detail::{EntryContactInfo, MyInfoEntryDetailEngine};

impl AppEngine {
    /// Persist settings toggle changes to Vauchi config (fixes HIGH-4).
    pub(super) fn persist_settings_toggle(&mut self, action: &UserAction) {
        if self.screen != AppScreen::Settings {
            return;
        }
        if let UserAction::SettingsToggled {
            component_id,
            item_id,
        } = action
            && component_id == "privacy"
        {
            let config = self.vauchi.config_mut();
            match item_id.as_str() {
                "delivery_receipts" => {
                    config.delivery_receipts_enabled = !config.delivery_receipts_enabled;
                }
                "suppress_presence" => {
                    config.suppress_presence = !config.suppress_presence;
                }
                _ => {}
            }
        }
    }

    /// Intercept settings item selection to route to proper sub-screens.
    pub(super) fn intercept_settings_action(
        &mut self,
        action: &UserAction,
    ) -> Option<ActionResult> {
        if self.screen != AppScreen::Settings {
            return None;
        }
        if let UserAction::ListItemSelected { item_id, .. } = action {
            match item_id.as_str() {
                "display_name" => {
                    let current_name = self
                        .vauchi
                        .own_card()
                        .ok()
                        .flatten()
                        .map(|c| c.display_name().to_string())
                        .unwrap_or_default();
                    let screen = self.navigate_to(AppScreen::FormDialog {
                        dialog_type: FormDialogType::EditName { current_name },
                    });
                    return Some(ActionResult::NavigateTo(screen));
                }
                "edit_profile" => {
                    let screen = self.navigate_to(AppScreen::MyInfo);
                    return Some(ActionResult::NavigateTo(screen));
                }
                "devices" => {
                    let screen = self.navigate_to(AppScreen::DeviceLinking);
                    return Some(ActionResult::NavigateTo(screen));
                }
                "duress_pin" => {
                    let screen = self.navigate_to(AppScreen::DuressPin);
                    return Some(ActionResult::NavigateTo(screen));
                }
                "relay_url" => {
                    let current_url = self.vauchi.config().relay.server_url.clone();
                    let screen = self.navigate_to(AppScreen::FormDialog {
                        dialog_type: FormDialogType::EditRelayUrl { current_url },
                    });
                    return Some(ActionResult::NavigateTo(screen));
                }
                "emergency_wipe" => {
                    let screen = self.navigate_to(AppScreen::EmergencyShred);
                    return Some(ActionResult::NavigateTo(screen));
                }
                // change_password: not yet implemented
                _ => {}
            }
        }
        None
    }

    /// Intercept add-field actions on MyInfo and Onboarding to open FormDialog.
    pub(super) fn intercept_add_field(&mut self, action: &UserAction) -> Option<ActionResult> {
        let action_id = match action {
            UserAction::ActionPressed { action_id } => action_id.as_str(),
            UserAction::ListItemSelected { item_id, .. } => item_id.as_str(),
            _ => return None,
        };

        if action_id != "add_field" && action_id != "add_entry" {
            return None;
        }

        // Only intercept on screens that support field addition
        if !matches!(self.screen, AppScreen::MyInfo | AppScreen::Onboarding) {
            return None;
        }

        let available_groups = self.available_groups();
        let screen = self.navigate_to(AppScreen::FormDialog {
            dialog_type: FormDialogType::AddField { available_groups },
        });
        Some(ActionResult::NavigateTo(screen))
    }

    /// Intercept entry detail actions before delegating to engine.
    pub(super) fn intercept_entry_detail_action(
        &mut self,
        field_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        match action {
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == "group_visibility" => {
                // Persist group visibility change
                let group_id = item_id.clone();
                let engine = self
                    .engine
                    .as_any_mut()
                    .and_then(|a| a.downcast_mut::<MyInfoEntryDetailEngine>());
                if let Some(engine) = engine {
                    // Find current state and toggle
                    let is_visible = engine
                        .groups
                        .iter()
                        .find(|(gid, _, _)| gid == &group_id)
                        .map(|(_, _, v)| *v)
                        .unwrap_or(false);
                    let new_visible = !is_visible;
                    let _ =
                        self.vauchi
                            .set_group_field_visibility(&group_id, field_id, new_visible);
                    // Update engine state
                    if let Some(entry) = engine
                        .groups
                        .iter_mut()
                        .find(|(gid, _, _)| gid == &group_id)
                    {
                        entry.2 = new_visible;
                    }
                    // Rebuild visible contacts
                    let all_groups = self.vauchi.list_groups().unwrap_or_default();
                    let mut visible_contacts = Vec::new();
                    let mut seen = std::collections::HashSet::new();
                    for g in &all_groups {
                        if g.is_field_visible(field_id) {
                            for cid in g.contacts() {
                                if seen.insert(cid.to_string()) {
                                    let name = self
                                        .vauchi
                                        .get_contact(cid)
                                        .ok()
                                        .flatten()
                                        .map(|c| c.display_name().to_string())
                                        .unwrap_or_else(|| "Unknown".into());
                                    visible_contacts.push(EntryContactInfo {
                                        contact_id: cid.to_string(),
                                        name,
                                        via_group: g.name().to_string(),
                                    });
                                }
                            }
                        }
                    }
                    engine.visible_contacts = visible_contacts;
                    // Invalidate MyInfo cache so it refreshes
                    self.engine_cache.remove(&AppScreen::MyInfo);
                    return Some(ActionResult::UpdateScreen(engine.current_screen()));
                }
            }
            UserAction::ActionPressed { action_id } if action_id == "edit" => {
                // Navigate to EditField form for this field
                if let Some(engine) = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<MyInfoEntryDetailEngine>())
                {
                    let label = engine.label.clone();
                    let value = engine.value.clone();
                    let note = engine.note.clone();
                    let screen = self.navigate_to(AppScreen::FormDialog {
                        dialog_type: FormDialogType::EditField {
                            field_id: field_id.to_string(),
                            field_label: label,
                            current_value: value,
                            current_note: note,
                        },
                    });
                    return Some(ActionResult::NavigateTo(screen));
                }
            }
            UserAction::ActionPressed { action_id } if action_id == "delete" => {
                if let Ok(Some(mut card)) = self.vauchi.own_card() {
                    // Find and clone the field before removing
                    if let Some(field) = card.fields().iter().find(|f| f.id() == field_id).cloned()
                    {
                        let _ = card.remove_field(field_id);
                        let _ = self.vauchi.update_own_card(&card);
                        self.pending_field_undo = Some((field_id.to_string(), field));
                    }
                }
                self.engine_cache.remove(&AppScreen::MyInfo);
                self.navigate_back();
                return Some(ActionResult::ShowToast {
                    message: "Field deleted".into(),
                    undo_action_id: Some(format!("undo_delete_field:{field_id}")),
                });
            }
            UserAction::ActionPressed { action_id } if action_id == "back" => {
                let screen = self.navigate_back();
                return Some(ActionResult::NavigateTo(screen));
            }
            _ => {}
        }
        None
    }

    /// Intercept personal note edits on the ContactDetail screen and persist them.
    ///
    /// When the user changes the `personal_note` EditableText component, the note
    /// is saved immediately as raw UTF-8 bytes via `Vauchi::save_personal_notes`.
    pub(super) fn intercept_personal_note_change(
        &mut self,
        contact_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        if let UserAction::TextChanged {
            component_id,
            value,
        } = action
            && component_id == "personal_note"
        {
            // Encryption handled at the storage layer (save_personal_notes encrypts
            // with the storage encryption key). Legacy plaintext rows are self-healed
            // on next load+save cycle. See: problems/2026-03-27-notes-encryption-gap.
            if let Err(e) = self
                .vauchi
                .save_personal_notes(contact_id, value.as_bytes())
            {
                let _ = e; // Silently ignore — UI already shows the field unchanged
            }
            self.invalidate_screen(&AppScreen::ContactDetail {
                contact_id: contact_id.to_string(),
            });
            return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
        }
        None
    }

    /// Intercept per-field note edits on the ContactDetail screen and persist them.
    ///
    /// When the user changes a `field_note:{field_id}` EditableText component,
    /// the note is saved immediately as raw UTF-8 bytes via
    /// `Vauchi::save_contact_field_note`.
    pub(super) fn intercept_field_note_change(
        &mut self,
        contact_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        if let UserAction::TextChanged {
            component_id,
            value,
        } = action
            && let Some(field_id) = component_id.strip_prefix("field_note:")
        {
            if let Err(e) =
                self.vauchi
                    .save_contact_field_note(contact_id, field_id, value.as_bytes())
            {
                let _ = e;
            }
            self.invalidate_screen(&AppScreen::ContactDetail {
                contact_id: contact_id.to_string(),
            });
            return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
        }
        None
    }

    /// Intercept the "exit-preview" action when MyInfo is in PreviewAs mode.
    ///
    /// Clears `preview_as_contact`, invalidates the MyInfo cache, and rebuilds
    /// MyInfo in normal edit mode.
    pub(super) fn intercept_exit_preview(&mut self, action: &UserAction) -> Option<ActionResult> {
        let UserAction::ActionPressed { action_id } = action else {
            return None;
        };
        if action_id != "exit-preview" {
            return None;
        }
        if self.screen != AppScreen::MyInfo {
            return None;
        }
        self.preview_as_contact = None;
        self.engine_cache.remove(&AppScreen::MyInfo);
        // Rebuild the engine in normal mode and update the current engine slot.
        let new_engine = Self::create_engine(&self.vauchi, &AppScreen::MyInfo, None);
        let _ = std::mem::replace(&mut self.engine, new_engine);
        Some(ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Intercept proposal_trusted toggle on ContactDetail and persist to storage.
    ///
    /// When the user toggles the "proposal_trusted" item in the "trust_permissions"
    /// SettingsGroup, we load the contact, flip the flag, save it, then let the
    /// engine update its own local state.
    pub(super) fn intercept_proposal_trust_toggle(
        &mut self,
        contact_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        let UserAction::SettingsToggled {
            component_id,
            item_id,
        } = action
        else {
            return None;
        };
        if component_id != "trust_permissions" || item_id != "proposal_trusted" {
            return None;
        }

        // Load the contact, flip the flag, save it back.
        if let Ok(Some(mut contact)) = self.vauchi.get_contact(contact_id) {
            let _ = contact.set_proposal_trusted(!contact.is_proposal_trusted());
            if let Err(e) = self.vauchi.update_contact(&contact) {
                let _ = e;
            }
        }

        // Let the engine update its in-memory state and return the new screen.
        self.engine
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<ContactDetailEngine>())
            .map(|engine| {
                engine.toggle_proposal_trusted();
                ActionResult::UpdateScreen(engine.current_screen())
            })
    }

    /// Handle undo actions (field delete restoration).
    pub(super) fn handle_undo(&mut self, action: &UserAction) -> Option<ActionResult> {
        if let UserAction::UndoPressed { action_id } = action
            && action_id.starts_with("undo_delete_field:")
        {
            if let Some((field_id, field)) = self.pending_field_undo.take() {
                let mut restored = false;
                if let Ok(Some(mut card)) = self.vauchi.own_card()
                    && card.add_field(field.clone()).is_ok()
                    && self.vauchi.update_own_card(&card).is_ok()
                {
                    restored = true;
                    self.engine_cache.remove(&AppScreen::MyInfo);
                }
                if !restored {
                    // Restore to undo buffer so retry is possible
                    self.pending_field_undo = Some((field_id, field));
                }
            }
            return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
        }
        None
    }
}
