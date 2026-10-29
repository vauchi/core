// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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
        if !self.other_device_names.is_empty() && self.wipe_other_devices.is_none() {
            self.step = GdprStep::ChooseWipeOtherDevices;
            return ActionResult::NavigateTo(self.build_screen());
        }
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

    pub(super) fn build_confirm_delete(&self) -> ScreenModel {
        let s = &self.deletion_summary;
        let mut items = vec![
            InfoItem {
                icon: Some("id_card".into()),
                title: self.t("privacy.delete.identity_title"),
                detail: self.t("privacy.delete.identity_detail"),
            },
            InfoItem {
                icon: Some("people".into()),
                title: self.t_count("privacy.delete.contacts_title", s.contact_count),
                detail: self.t("privacy.delete.contacts_detail"),
            },
            InfoItem {
                icon: Some("cloud".into()),
                title: self.t("privacy.delete.relay_title"),
                detail: self.t("privacy.delete.relay_detail"),
            },
            InfoItem {
                icon: Some("key".into()),
                title: self.t("privacy.delete.keychain_title"),
                detail: self.t("privacy.delete.keychain_detail"),
            },
        ];

        if s.device_count > 1 {
            items.push(InfoItem {
                icon: Some("devices".into()),
                title: self.t_count("privacy.delete.devices_title", s.device_count - 1),
                detail: self.t("privacy.delete.devices_detail"),
            });
        }

        items.push(InfoItem {
            icon: Some("warning".into()),
            title: self.t("privacy.delete.irreversible_title"),
            detail: self.t("privacy.delete.irreversible_detail"),
        });

        ScreenModel {
            screen_id: "delete_identity_summary".into(),
            title: self.t("privacy.delete_identity"),
            subtitle: Some(self.t("privacy.delete.review_subtitle")),
            components: vec![Component::InfoPanel {
                id: "deletion_summary".into(),
                icon: Some("warning".into()),
                title: self.t("privacy.delete.list_title"),
                items,
                a11y: None,
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: "confirm_delete".into(),
                    label: self.t("privacy.delete_identity"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    // Screen-reader context for the most irreversible
                    // action in the app. VoiceOver / TalkBack announce
                    // the explicit label rather than the shorter visible
                    // "Delete Identity" button text, then read the hint
                    // as the usage guidance.
                    a11y: Some(A11y {
                        label: Some(self.t("privacy.delete.a11y_confirm")),
                        hint: Some(self.t("privacy.delete.a11y_confirm_hint")),
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
            progress: None,
            ..Default::default()
        }
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
