// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Cross-cutting `AppEngine` action intercepts that belong to no single
//! feature: the app-wide info-overlay resolver and the undo dispatcher.
//! Feature-scoped intercepts live in sibling `intercept_*.rs` modules
//! (settings, card, contact_detail, recovery, contact_lifecycle,
//! annotations); all are dispatched from `dispatch.rs`/`mod.rs`.

use super::AppEngine;
use super::AppScreen;
use crate::i18n::Locale;
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::info_content;

impl AppEngine {
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
}
