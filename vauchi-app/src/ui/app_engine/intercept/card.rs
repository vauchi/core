// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Own-card editing intercepts: avatar edit, add-field dialog, entry-detail
//! visibility/edit/delete, and exit-preview. Split out of `intercept.rs`
//! (cohesion). `impl AppEngine` methods, dispatched from `mod.rs`/`dispatch.rs`.

use super::super::AppEngine;
use super::super::AppScreen;
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::form_dialog::FormDialogType;
use crate::ui::my_info_entry_detail::EntryContactInfo;

impl AppEngine {
    /// Intercept "edit_avatar" action on MyInfo to navigate to AvatarEditor.
    pub(in crate::ui::app_engine) fn intercept_edit_avatar(
        &mut self,
        action: &UserAction,
    ) -> Option<ActionResult> {
        if !matches!(self.screen, AppScreen::MyInfo) {
            return None;
        }
        if let UserAction::ActionPressed { action_id } = action
            && action_id == "edit_avatar"
        {
            let screen = self.navigate_to(AppScreen::AvatarEditor);
            return Some(ActionResult::NavigateTo(screen));
        }
        None
    }

    /// Intercept add-field actions on MyInfo and Onboarding to open FormDialog.
    pub(in crate::ui::app_engine) fn intercept_add_field(
        &mut self,
        action: &UserAction,
    ) -> Option<ActionResult> {
        let action_id = match action {
            UserAction::ActionPressed { action_id } => action_id.as_str(),
            UserAction::ListItemSelected { item_id, .. } => item_id.as_str(),
            _ => return None,
        };

        if action_id != "add_field" && action_id != "add_entry" && action_id != "add_social" {
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
    pub(in crate::ui::app_engine) fn intercept_entry_detail_action(
        &mut self,
        field_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        match action {
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == "entry_visibility" && item_id == "shown" => {
                // Persist the unassigned entry's all-contacts toggle. The
                // engine already flipped its local state; storage is the
                // source of truth for the new value.
                let new_shown = !self
                    .vauchi
                    .own_card()
                    .ok()
                    .flatten()
                    .map(|c| c.is_field_shown(field_id))
                    .unwrap_or(false);
                // best-effort like the group arm: a failed save leaves
                // storage unchanged and the next rebuild shows truth
                #[allow(clippy::let_underscore_must_use)]
                let _ = self.vauchi.set_field_shown(field_id, new_shown);
                // Invalidate MyInfo cache so it refreshes; the engine's own
                // handler renders the flipped toggle.
                self.engine_cache.remove(&AppScreen::MyInfo);
            }
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == "group_visibility" => {
                // Persist group visibility change
                let group_id = item_id.clone();
                let groups = match self.engine.engine_output() {
                    Some(crate::ui::EngineOutput::MyInfoEntryDetail { groups, .. }) => groups,
                    other => {
                        tracing::warn!(?other, "group toggle without MyInfoEntryDetail output");
                        return None;
                    }
                };
                let is_visible = groups
                    .iter()
                    .find(|(gid, _, _)| gid == &group_id)
                    .map(|(_, _, v)| *v)
                    .unwrap_or(false);
                let new_visible = !is_visible;
                // best-effort: per-group visibility toggle; failure
                // leaves the storage state unchanged and the screen
                // rebuild below will reflect actual state
                #[allow(clippy::let_underscore_must_use)]
                let _ = self
                    .vauchi
                    .set_group_field_visibility(&group_id, field_id, new_visible);
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
                if self
                    .engine
                    .apply_update(crate::ui::EngineUpdate::MyInfoEntryDetail(
                        crate::ui::MyInfoEntryDetailUpdate::GroupVisibility {
                            group_id,
                            visible: new_visible,
                            visible_contacts,
                        },
                    ))
                {
                    // Invalidate MyInfo cache so it refreshes
                    self.engine_cache.remove(&AppScreen::MyInfo);
                    return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
                }
            }
            UserAction::ActionPressed { action_id } if action_id == "edit" => {
                // Navigate to EditField form for this field
                if let Some(crate::ui::EngineOutput::MyInfoEntryDetail {
                    label, value, note, ..
                }) = self.engine.engine_output()
                {
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
                // Field delete is user-visible; surface failures as ShowAlert
                // so the user doesn't see "Field deleted" when the row is
                // still in storage.
                if let Ok(Some(mut card)) = self.vauchi.own_card() {
                    // Find and clone the field before removing
                    if let Some(field) = card.fields().iter().find(|f| f.id() == field_id).cloned()
                    {
                        if let Err(e) = card.remove_field(field_id) {
                            return Some(ActionResult::ShowAlert {
                                title: self.t("contacts.delete_failed_title"),
                                message: format!("{e}"),
                            });
                        }
                        if let Err(e) = self.vauchi.update_own_card(&card) {
                            return Some(ActionResult::ShowAlert {
                                title: self.t("contacts.delete_failed_title"),
                                message: format!("{e}"),
                            });
                        }
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

    /// Intercept the "exit-preview" action when MyInfo is in PreviewAs mode.
    ///
    /// Clears `preview_as_contact`, invalidates the MyInfo cache, and rebuilds
    /// MyInfo in normal edit mode.
    pub(in crate::ui::app_engine) fn intercept_exit_preview(
        &mut self,
        action: &UserAction,
    ) -> Option<ActionResult> {
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
        let new_engine = Self::create_engine(
            &self.vauchi,
            &AppScreen::MyInfo,
            None,
            &self.device_capabilities,
            &self.transport_readiness,
            &self.render_context,
            &self.pending_exchange_groups,
            self.glance_display_qr.as_deref(),
        );
        // best-effort discard: we don't need the old engine value
        #[allow(clippy::let_underscore_must_use)]
        let _ = std::mem::replace(&mut self.engine, new_engine);
        Some(ActionResult::UpdateScreen(self.engine.current_screen()))
    }
}
