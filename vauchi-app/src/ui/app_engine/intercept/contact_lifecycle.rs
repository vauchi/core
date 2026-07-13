// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact-lifecycle intercepts: archive (with undo), duplicate merge,
//! duplicate dismiss, and decoy-contact add/delete. Split out of
//! `intercept.rs` (cohesion). `impl AppEngine` methods, dispatched from
//! `dispatch.rs`.

use super::super::AppEngine;
use super::super::AppScreen;
use crate::ui::action::{ActionResult, UserAction};

impl AppEngine {
    /// Intercept delete/archive actions on ContactDetail, perform the side effect,
    /// store undo state, and return a ShowToast with the undo action ID.
    pub(in crate::ui::app_engine) fn intercept_contact_delete_archive(
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
                self.pending_contact_undo = Some(super::super::PendingContactUndo::Archive);
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
    pub(in crate::ui::app_engine) fn intercept_merge_action(&mut self) -> Option<ActionResult> {
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

    /// Intercept the "dismiss" action on the ContactDuplicates screen.
    ///
    /// Reads the engine's `selected_pair_index`, calls
    /// `vauchi.dismiss_duplicate(id1, id2)` for that pair, then refreshes
    /// the screen so the dismissed pair drops out of the list. Returns a
    /// non-blocking toast so the user knows the action took effect.
    ///
    /// Returns `None` when no pairs exist or no selection is recorded.
    pub(in crate::ui::app_engine) fn intercept_dismiss_duplicate_action(
        &mut self,
    ) -> Option<ActionResult> {
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

    /// Persist add/delete actions on the DecoyContacts screen.
    ///
    /// Mutations bypass the engine's own state (the engine is a humble
    /// renderer); the intercept reads the engine's pending fields, calls
    /// the storage API, then rebuilds the engine with the fresh list so
    /// the user sees the updated screen without leaving it.
    pub(in crate::ui::app_engine) fn intercept_decoy_contacts_action(
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
