// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Block and unblock on ContactDetail (ADR-056, private#889). The engine
//! asks and holds the wipe choice; the confirm lands here, where Vauchi
//! blocks, and the toast says what that did.

use super::super::{AppEngine, AppScreen};
use crate::i18n::get_string_with_args;
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::{ContactDetailUpdate, EngineOutput, EngineUpdate};
use vauchi_core::BlockReport;

impl AppEngine {
    pub(in crate::ui::app_engine) fn intercept_block_contact(
        &mut self,
        contact_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        let UserAction::ActionPressed { action_id } = action else {
            return None;
        };
        match action_id.as_str() {
            "confirm_block_contact" => Some(self.block_contact(contact_id)),
            "unblock_contact" => Some(self.unblock_contact(contact_id)),
            _ => None,
        }
    }

    fn block_contact(&mut self, contact_id: &str) -> ActionResult {
        let wipe_their_copy = matches!(
            self.engine.engine_output(),
            Some(EngineOutput::ContactDetail {
                wipe_their_copy: true,
                ..
            })
        );
        let name = self.contact_display_name(contact_id);
        match self
            .vauchi
            .block_contact_and_report(contact_id, wipe_their_copy)
        {
            Ok(report) => {
                self.set_blocked_on_screen(true);
                ActionResult::ShowToast {
                    message: self.blocked_toast(&name, wipe_their_copy, report),
                    undo_action_id: None,
                    undo_label: None,
                }
            }
            Err(error) => ActionResult::ShowAlert {
                title: self.t("error.title"),
                message: error.to_string(),
            },
        }
    }

    fn unblock_contact(&mut self, contact_id: &str) -> ActionResult {
        let name = self.contact_display_name(contact_id);
        match self.vauchi.unblock_contact(contact_id) {
            Ok(()) => {
                self.set_blocked_on_screen(false);
                ActionResult::ShowToast {
                    message: self.t_name("contacts.toast_unblocked", &name),
                    undo_action_id: None,
                    undo_label: None,
                }
            }
            Err(error) => ActionResult::ShowAlert {
                title: self.t("error.title"),
                message: error.to_string(),
            },
        }
    }

    fn blocked_toast(&self, name: &str, wipe_asked: bool, report: BlockReport) -> String {
        let key = match (wipe_asked, report.wipe_queued) {
            (false, _) => "contacts.toast_blocked",
            (true, true) => "contacts.toast_blocked_wiped",
            (true, false) => "contacts.toast_blocked_wipe_failed",
        };
        let mut message = self.t_name(key, name);
        if report.removed_from_emergency_contacts {
            message.push(' ');
            message.push_str(&self.t_name("contacts.toast_removed_from_emergency", name));
        }
        message
    }

    fn set_blocked_on_screen(&mut self, is_blocked: bool) {
        let _ = self.engine.apply_update(EngineUpdate::ContactDetail(
            ContactDetailUpdate::SetBlocked(is_blocked),
        ));
        self.engine_cache.remove(&AppScreen::EmergencyBroadcast);
    }

    fn contact_display_name(&self, contact_id: &str) -> String {
        self.vauchi
            .get_contact(contact_id)
            .ok()
            .flatten()
            .map(|c| c.display_name().to_string())
            .unwrap_or_default()
    }

    fn t_name(&self, key: &str, name: &str) -> String {
        get_string_with_args(
            self.render_context.resolved_locale(),
            key,
            &[("name", name)],
        )
    }
}
