// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Action interception methods for `AppEngine` — settings routing, entry detail
//! actions, add-field dialog, and undo support.

use super::AppEngine;
use super::AppScreen;
use crate::i18n::Locale;
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::form_dialog::FormDialogType;
use crate::ui::info_content;
use crate::ui::my_info_entry_detail::EntryContactInfo;

impl AppEngine {
    /// Intercept "edit_avatar" action on MyInfo to navigate to AvatarEditor.
    pub(super) fn intercept_edit_avatar(&mut self, action: &UserAction) -> Option<ActionResult> {
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
    pub(super) fn intercept_add_field(&mut self, action: &UserAction) -> Option<ActionResult> {
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
    pub(super) fn intercept_entry_detail_action(
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
        // best-effort: the engine's optimistic flip below will revert
        // on the next ContactDetail engine read if storage didn't change.
        if let Ok(Some(mut contact)) = self.vauchi.get_contact(contact_id) {
            #[allow(clippy::let_underscore_must_use)]
            let _ = contact.set_proposal_trusted(!contact.is_proposal_trusted());
            if let Err(e) = self.vauchi.update_contact(&contact) {
                // Drop the error explicitly — same best-effort rationale.
                #[allow(clippy::let_underscore_must_use)]
                let _ = e;
            }
        }

        // Let the engine update its in-memory state and return the new screen.
        self.engine
            .apply_update(crate::ui::EngineUpdate::ContactDetail(
                crate::ui::ContactDetailUpdate::ToggleProposalTrusted,
            ))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Intercept the recovery-trust SettingsToggled on ContactDetail and
    /// persist to storage. Mirror of `intercept_proposal_trust_toggle`,
    /// added 2026-04-28 for the Pair 3 (ContactDetail) Pure Humble UI
    /// retirement work.
    pub(super) fn intercept_recovery_trust_toggle(
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
        if component_id != "recovery_permissions" || item_id != "recovery_trusted" {
            return None;
        }

        // Toggle recovery trust via the canonical Vauchi API. Errors are
        // swallowed here — the engine's optimistic flip will be reverted
        // on the next AppScreen::ContactDetail engine read if storage
        // didn't actually change.
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.vauchi.toggle_recovery_trust(contact_id);

        self.engine
            .apply_update(crate::ui::EngineUpdate::ContactDetail(
                crate::ui::ContactDetailUpdate::ToggleRecoveryTrusted,
            ))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Intercept hide/unhide toggle on ContactDetail and persist to storage.
    pub(super) fn intercept_hide_toggle(
        &mut self,
        contact_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        let UserAction::ActionPressed { action_id } = action else {
            return None;
        };
        if action_id != "toggle_hidden" {
            return None;
        }

        let is_hidden = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::ContactDetail { is_hidden }) => is_hidden,
            other => {
                tracing::warn!(?other, "hide toggle without ContactDetail output");
                false
            }
        };

        // best-effort: screen rebuild below reflects actual storage
        // state; an optimistic flip that diverges is recovered on next read
        if is_hidden {
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.vauchi.unhide_contact(contact_id);
        } else {
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.vauchi.hide_contact(contact_id);
        }

        self.engine
            .apply_update(crate::ui::EngineUpdate::ContactDetail(
                crate::ui::ContactDetailUpdate::ToggleHidden,
            ))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Intercept delete/archive actions on ContactDetail, perform the side effect,
    /// store undo state, and return a ShowToast with the undo action ID.
    pub(super) fn intercept_contact_delete_archive(
        &mut self,
        contact_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        let UserAction::ActionPressed { action_id } = action else {
            return None;
        };

        match action_id.as_str() {
            // delete_contact is now handled by the engine (InlineConfirm flow).
            // The actual deletion happens in handle_completion for ContactDetail.
            "delete_contact" | "confirm_delete_contact" | "cancel_delete_contact" => None,
            "archive_contact" => {
                // best-effort: contact_list rebuild reflects truth on next
                // open; the canonical archive path is `apply_contact_action`
                // which surfaces ShowAlert on failure
                #[allow(clippy::let_underscore_must_use)]
                let _ = self.vauchi.archive_contact(contact_id);
                self.pending_contact_undo = Some(super::PendingContactUndo::Archive);
                self.engine_cache.remove(&AppScreen::Contacts);
                self.navigate_back();
                Some(ActionResult::ShowToast {
                    message: "Contact archived".into(),
                    undo_action_id: Some(format!("undo_archive_contact:{contact_id}")),
                })
            }
            _ => None,
        }
    }

    /// Intercept the "merge" action on the ContactDuplicates screen.
    ///
    /// Reads the engine's `selected_pair_index` to pick the user-selected
    /// pair (the merge button is gated on selection so this should always
    /// be `Some`). Falls back to pair `[0]` if no selection is recorded —
    /// this only happens if a frontend skips the selection step entirely.
    ///
    /// Cross-kind pairs (one imported + one exchanged) cannot be merged —
    /// `vauchi.merge_contacts` would reject with `InvalidState`. We catch
    /// that case here and surface a `ShowAlert` instead of navigating to
    /// the merge preview screen.
    ///
    /// Returns `None` if there are no pairs at all.
    pub(super) fn intercept_merge_action(&mut self) -> Option<ActionResult> {
        let pairs = self.vauchi.find_duplicates().unwrap_or_default();
        if pairs.is_empty() {
            return None;
        }

        let selected_idx = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::DuplicateDetection {
                selected_pair_index,
            }) => selected_pair_index.unwrap_or(0),
            other => {
                tracing::warn!(?other, "merge without DuplicateDetection output");
                0
            }
        };

        let pair = pairs.get(selected_idx).cloned()?;

        let primary = self.vauchi.get_contact(&pair.id1).ok().flatten()?;
        let secondary = self.vauchi.get_contact(&pair.id2).ok().flatten()?;

        // Cross-kind pairs can't merge — surface a clear alert instead of
        // navigating to a merge preview the user can't actually confirm.
        if primary.is_imported() != secondary.is_imported() {
            return Some(ActionResult::ShowAlert {
                title: self.t("contact_merge.cant_merge_title"),
                message: "These contacts can't be merged because one was \
                          exchanged in person and the other was imported. \
                          Delete the imported one if it duplicates the \
                          exchanged contact."
                    .into(),
            });
        }

        let primary_name = primary.display_name().to_string();
        let secondary_name = secondary.display_name().to_string();
        let primary_fields: Vec<String> = primary
            .card()
            .fields()
            .iter()
            .map(|f| format!("{}: {}", f.label(), f.value()))
            .collect();
        let secondary_fields: Vec<String> = secondary
            .card()
            .fields()
            .iter()
            .map(|f| format!("{}: {}", f.label(), f.value()))
            .collect();

        self.pending_merge = Some((pair.id1, pair.id2));

        let screen = self.navigate_to(AppScreen::ContactMerge {
            primary_name,
            primary_fields,
            secondary_name,
            secondary_fields,
        });
        Some(ActionResult::NavigateTo(screen))
    }

    /// Intercept the "create_claim" action on the Recovery screen
    /// (EnterOldKey step).
    ///
    /// Reads the engine's `old_key_input` (a hex-encoded public key),
    /// passes it to `Vauchi::create_recovery_claim_hex_b64`, then either
    /// advances the engine to ShowGeneratedClaim (success) or attaches
    /// a validation error to the input (failure). Returns `UpdateScreen`
    /// so the rendered screen reflects the new engine state.
    pub(super) fn intercept_create_claim_action(&mut self) -> Option<ActionResult> {
        let old_key = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::Recovery { old_key_input }) => {
                old_key_input.trim().to_string()
            }
            _ => return None,
        };

        let result = self.vauchi.create_recovery_claim_hex_b64(&old_key);

        let update = match result {
            Ok(claim_b64) => crate::ui::RecoveryUpdate::ClaimGenerated(claim_b64),
            Err(e) => crate::ui::RecoveryUpdate::ClaimCreateError(format!("{e}")),
        };
        self.engine
            .apply_update(crate::ui::EngineUpdate::Recovery(update))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Intercept the "verify_claim" action on the RecoveryHelp screen.
    ///
    /// Reads the user-pasted claim payload from the engine, base64-decodes
    /// and parses it via `RecoveryClaim::from_bytes`, then either advances
    /// the engine to the ConfirmVoucher step (success) or attaches a
    /// validation error to the input (failure). Returns `UpdateScreen` so
    /// the rendered screen reflects the new engine state.
    pub(super) fn intercept_verify_claim_action(&mut self) -> Option<ActionResult> {
        let claim_input = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::RecoveryHelp { claim_input }) => {
                claim_input.trim().to_string()
            }
            _ => return None,
        };

        let parse_result = self.vauchi.parse_recovery_claim_b64(&claim_input);

        let update = match parse_result {
            Ok(claim) => crate::ui::RecoveryHelpUpdate::ParsedClaim(
                crate::ui::recovery_help::ParsedClaimSummary {
                    old_pk_hex: hex::encode::<&[u8]>(claim.old_pk().as_ref()),
                    new_pk_hex: hex::encode::<&[u8]>(claim.new_pk().as_ref()),
                    is_expired: claim.is_expired(self.vauchi.clock().unix_seconds()),
                },
            ),
            Err(e) => crate::ui::RecoveryHelpUpdate::ClaimParseError(format!("Invalid claim: {e}")),
        };
        self.engine
            .apply_update(crate::ui::EngineUpdate::RecoveryHelp(update))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Intercept the "create_voucher" action on the RecoveryHelp screen.
    ///
    /// Re-decodes the claim from the engine input and signs a voucher with
    /// the local identity's signing keypair via
    /// `Vauchi::create_voucher_from_claim_b64` (mirrors the existing
    /// mobile platform `create_recovery_voucher` flow — no guardian
    /// token, no relay round-trip). Stores the base64 voucher payload on
    /// the engine so the ShowVoucher screen can render it for the user
    /// to share.
    pub(super) fn intercept_create_voucher_action(&mut self) -> Option<ActionResult> {
        let claim_input = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::RecoveryHelp { claim_input }) => {
                claim_input.trim().to_string()
            }
            _ => return None,
        };

        match self.vauchi.create_voucher_from_claim_b64(&claim_input) {
            Ok(voucher_b64) => self
                .engine
                .apply_update(crate::ui::EngineUpdate::RecoveryHelp(
                    crate::ui::RecoveryHelpUpdate::VoucherData(voucher_b64),
                ))
                .then(|| ActionResult::UpdateScreen(self.engine.current_screen())),
            Err(e) => Some(ActionResult::ShowAlert {
                title: self.t("recovery_help.voucher_error_title"),
                message: format!("{e}"),
            }),
        }
    }

    /// Intercept the "dismiss" action on the ContactDuplicates screen.
    ///
    /// Reads the engine's `selected_pair_index`, calls
    /// `vauchi.dismiss_duplicate(id1, id2)` for that pair, then refreshes
    /// the screen so the dismissed pair drops out of the list. Returns a
    /// non-blocking toast so the user knows the action took effect.
    ///
    /// Returns `None` when no pairs exist or no selection is recorded.
    pub(super) fn intercept_dismiss_duplicate_action(&mut self) -> Option<ActionResult> {
        let pairs = self.vauchi.find_duplicates().unwrap_or_default();
        if pairs.is_empty() {
            return None;
        }

        let selected_idx = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::DuplicateDetection {
                selected_pair_index,
            }) => selected_pair_index?,
            _ => return None,
        };

        let pair = pairs.get(selected_idx).cloned()?;
        // best-effort: duplicate dismissal is idempotent; if the row was
        // already dismissed (or storage faults), the engine recreate below
        // will fetch the actual list
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.vauchi.dismiss_duplicate(&pair.id1, &pair.id2);

        // Recreate the engine so the dismissed pair drops from the list
        // and the selection state resets.
        self.engine_cache.remove(&AppScreen::ContactDuplicates);
        let screen = self.screen.clone();
        self.engine = Self::create_engine(
            &self.vauchi,
            &screen,
            self.preview_as_contact.as_deref(),
            &self.device_capabilities,
            &self.transport_readiness,
            &self.render_context,
            &self.pending_exchange_groups,
            self.glance_display_qr.as_deref(),
        );

        Some(ActionResult::ShowToast {
            message: "Duplicate dismissed".into(),
            undo_action_id: None,
        })
    }

    /// Intercept `InfoRequested` and resolve the key to localized help content.
    ///
    /// Returns `ShowInfoOverlay` when the key is known, or `UpdateScreen`
    /// with the current screen when the key is unknown. This intercept runs
    /// before delegation to child engines so every screen in the app gets
    /// (i) icon support without each engine needing to handle it.
    pub(super) fn intercept_info_requested(&mut self, action: &UserAction) -> Option<ActionResult> {
        let UserAction::InfoRequested { key } = action else {
            return None;
        };
        if let Some((title, body)) = info_content::resolve_info_key(key, Locale::English) {
            return Some(ActionResult::ShowInfoOverlay { title, body });
        }
        // Unknown key: refresh the current screen rather than silently swallowing the tap.
        Some(ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Handle undo actions (field delete restoration, contact delete/archive undo).
    pub(super) fn handle_undo(&mut self, action: &UserAction) -> Option<ActionResult> {
        let UserAction::UndoPressed { action_id } = action else {
            return None;
        };

        if action_id.starts_with("undo_delete_field:") {
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

        // undo_delete_contact removed: delete is now irrevocable (InlineConfirm).

        if let Some(contact_id) = action_id.strip_prefix("undo_archive_contact:") {
            // best-effort undo: user can retry by archiving again if this
            // failed; the navigation below shows the actual state
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.vauchi.unarchive_contact(contact_id);
            self.pending_contact_undo = None;
            self.engine_cache.remove(&AppScreen::Contacts);
            let screen = self.navigate_to(AppScreen::ContactDetail {
                contact_id: contact_id.to_string(),
            });
            return Some(ActionResult::NavigateTo(screen));
        }

        // Swipe-undo from Contacts list: restore and stay on the list.
        if let Some(contact_id) = action_id.strip_prefix("undo_hide_contact:") {
            // best-effort undo (see undo_archive comment above)
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.vauchi.unhide_contact(contact_id);
            self.engine_cache.remove(&AppScreen::Contacts);
            return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
        }

        if let Some(contact_id) = action_id.strip_prefix("undo_delete_contact:") {
            // best-effort undo (see undo_archive comment above)
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.vauchi.undo_delete_imported_contact(contact_id);
            self.engine_cache.remove(&AppScreen::Contacts);
            return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
        }

        None
    }

    /// Persist add/delete actions on the DecoyContacts screen.
    ///
    /// Mutations bypass the engine's own state (the engine is a humble
    /// renderer); the intercept reads the engine's pending fields, calls
    /// the storage API, then rebuilds the engine with the fresh list so
    /// the user sees the updated screen without leaving it.
    pub(super) fn intercept_decoy_contacts_action(
        &mut self,
        action: &UserAction,
    ) -> Option<ActionResult> {
        if self.screen != AppScreen::DecoyContacts {
            return None;
        }
        let UserAction::ActionPressed { action_id } = action else {
            return None;
        };

        match action_id.as_str() {
            "add_decoy" => {
                let name = match self.engine.engine_output() {
                    Some(crate::ui::EngineOutput::DecoyContacts { new_name, .. }) => {
                        new_name.trim().to_string()
                    }
                    other => {
                        tracing::warn!(?other, "add_decoy without DecoyContacts output");
                        String::new()
                    }
                };
                if name.is_empty() {
                    return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
                }
                // Time-based ID matches the legacy mobile-platform helper.
                // Storage uses INSERT OR REPLACE keyed by id, so collisions
                // (rapid double-tap) overwrite — acceptable for a fake list.
                let id = format!(
                    "decoy-{}",
                    self.vauchi
                        .clock()
                        .now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0)
                );
                let card = vauchi_core::contact_card::ContactCard::new(&name);
                // best-effort: refresh_decoy_engine below re-reads storage,
                // so a failed insert shows up as the new card not appearing
                #[allow(clippy::let_underscore_must_use)]
                let _ = self.vauchi.add_decoy_contact(&id, &name, &card);
                self.refresh_decoy_engine();
                Some(ActionResult::UpdateScreen(self.engine.current_screen()))
            }
            "confirm_delete_decoy" => {
                let pending = match self.engine.engine_output() {
                    Some(crate::ui::EngineOutput::DecoyContacts {
                        pending_delete_id, ..
                    }) => pending_delete_id,
                    _ => None,
                };
                if let Some(id) = pending {
                    // best-effort: refresh_decoy_engine below re-reads
                    // storage; a failed delete shows up as the row staying
                    #[allow(clippy::let_underscore_must_use)]
                    let _ = self.vauchi.remove_decoy_contact(&id);
                }
                self.refresh_decoy_engine();
                Some(ActionResult::UpdateScreen(self.engine.current_screen()))
            }
            _ => None,
        }
    }

    fn refresh_decoy_engine(&mut self) {
        self.engine_cache.remove(&AppScreen::DecoyContacts);
        let screen = AppScreen::DecoyContacts;
        self.engine = Self::create_engine(
            &self.vauchi,
            &screen,
            self.preview_as_contact.as_deref(),
            &self.device_capabilities,
            &self.transport_readiness,
            &self.render_context,
            &self.pending_exchange_groups,
            self.glance_display_qr.as_deref(),
        );
    }
}
