// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Scheduling a deletion asks whether recovery stays possible (#711): the
//! relay keeps the encrypted list of trusted contacts for up to 365 days,
//! or the purge erases it with everything else. With nobody trusted there
//! is nothing to keep, so the deletion erases everything without asking.

use super::*;

impl GdprEngine {
    /// Someone is trusted for recovery, so deleting asks what to keep.
    pub fn with_trusted_contacts(mut self, any: bool) -> Self {
        self.has_trusted_contacts = any;
        self
    }

    pub(super) fn confirm_delete(&mut self) -> ActionResult {
        if self.has_trusted_contacts {
            self.step = GdprStep::ChooseRecoveryKeep;
            return ActionResult::NavigateTo(self.build_screen());
        }
        self.last_action = Some("delete_erasing_everything".into());
        ActionResult::Complete
    }

    pub(super) fn choose_recovery(&mut self, action_id: &str) -> ActionResult {
        self.last_action = match action_id {
            "keep_recovery" => Some("delete".into()),
            "erase_everything" => Some("delete_erasing_everything".into()),
            _ => return ActionResult::UpdateScreen(self.build_screen()),
        };
        ActionResult::Complete
    }

    pub(super) fn build_recovery_choice(&self) -> ScreenModel {
        let body = self.t("privacy.delete.recovery_choice_body");
        ScreenModel {
            screen_id: "delete_recovery_choice".into(),
            title: self.t("privacy.delete.recovery_choice_title"),
            components: vec![Component::Text {
                id: "delete_recovery_choice_body".into(),
                content: body.clone(),
                style: TextStyle::Body,
                a11y: Some(A11y::labeled(body)),
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: "keep_recovery".into(),
                    label: self.t("privacy.delete.keep_recovery"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "erase_everything".into(),
                    label: self.t("privacy.delete.erase_everything"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y {
                        label: Some(self.t("privacy.delete.erase_everything")),
                        hint: Some(self.t("privacy.delete.erase_everything_hint")),
                        role: None,
                    }),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            ..Default::default()
        }
    }
}
