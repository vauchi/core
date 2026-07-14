// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact- and group-screen completion handlers for `AppEngine`.
//!
//! Split out of `completion.rs` to keep that dispatcher file under the
//! size limit. These are the `complete_<screen>` handlers for the
//! contact-detail / contact-edit / contact-merge / group screens; the
//! `routing::handle_completion` dispatcher delegates here the same way it
//! does for the handlers that remain in `completion.rs`.

use super::{AppEngine, AppScreen};
use crate::ui::action::ActionResult;

impl AppEngine {
    /// Contact-detail complete: hard-delete the imported contact, navigate back.
    pub(super) fn complete_contact_detail(&mut self, contact_id: &str) -> ActionResult {
        // InlineConfirm → hard delete the imported contact and navigate back.
        // best-effort: plain "back" also routes through this
        // completion handler (no pending-confirm flag yet), so a
        // "not found" / non-imported contact is expected and we
        // navigate-back regardless. Propagating would force every
        // plain back press to surface ShowAlert — the user-intent
        // gate belongs in the InlineConfirm engine, not here.
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.vauchi.hard_delete_imported_contact(contact_id);
        self.engine_cache.remove(&AppScreen::Contacts);
        let screen = self.navigate_back();
        ActionResult::NavigateTo(screen)
    }

    /// Contact-edit complete: persist the edited display name as a local
    /// nickname override (`Vauchi::set_contact_display_name`), then return to
    /// the contact detail. The edited name is read off the engine BEFORE
    /// `navigate_back` replaces it (same ordering constraint as
    /// `complete_exchange`).
    pub(super) fn complete_contact_edit(&mut self, contact_id: &str) -> ActionResult {
        let edited_name = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::ContactEdit { display_name }) => Some(display_name),
            other => {
                tracing::warn!(?other, "contact-edit completion without ContactEdit output");
                None
            }
        };

        if let Some(name) = edited_name
            && let Err(e) = self.vauchi.set_contact_display_name(contact_id, &name)
        {
            return ActionResult::ShowAlert {
                title: self.t("contact_edit.error_title"),
                message: format!("{e}"),
            };
        }
        self.engine_cache.remove(&AppScreen::Contacts);
        self.engine_cache.remove(&AppScreen::ContactDetail {
            contact_id: contact_id.to_string(),
        });
        let screen = self.navigate_back();
        ActionResult::NavigateTo(screen)
    }

    /// Group-detail complete: delete the group, navigate back.
    pub(super) fn complete_group_detail(&mut self, group_id: &str) -> ActionResult {
        if let Err(e) = self.vauchi.delete_group(group_id) {
            return ActionResult::ShowAlert {
                title: self.t("group_detail.delete_error_title"),
                message: format!("{e}"),
            };
        }
        self.engine_cache.remove(&AppScreen::Groups);
        let screen = self.navigate_back();
        ActionResult::NavigateTo(screen)
    }

    /// Groups list complete: defensive navigate-back (deletion lives on
    /// GroupDetail, where the target group is unambiguous).
    pub(super) fn complete_groups(&mut self) -> ActionResult {
        let screen = self.navigate_back();
        ActionResult::NavigateTo(screen)
    }

    /// Contact-merge complete: merge the pending pair if present.
    pub(super) fn complete_contact_merge(&mut self) -> ActionResult {
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
                            undo_label: None,
                        }
                    }
                    Err(e) => ActionResult::ShowAlert {
                        title: self.t("contact_merge.error_title"),
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
}
