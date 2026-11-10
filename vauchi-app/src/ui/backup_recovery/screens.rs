// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The backup wizard's screens, one builder per step.

use super::*;

impl BackupRecoveryEngine {
    pub(super) fn last_backup_line(&self) -> String {
        match self.last_backup {
            Some(then) => get_string_with_args(
                self.locale,
                "backup.manage.last_backup",
                &[(
                    "when",
                    &crate::relative_time::format_relative_time(self.now, then, self.locale),
                )],
            ),
            None => self.t("backup.manage.last_backup_never"),
        }
    }

    pub(super) fn level_choice(&self) -> Component {
        let detail = match self.level {
            BackupLevel::Full => self.t("backup.wizard.full_detail"),
            BackupLevel::IdentityOnly => self.t("backup.wizard.identity_only_detail"),
        };
        let toggle_label = match self.level {
            BackupLevel::Full => self.t("backup.wizard.level_full"),
            BackupLevel::IdentityOnly => self.t("backup.wizard.level_identity_only"),
        };
        Component::ToggleList {
            id: "backup_level".into(),
            label: self.t("backup.wizard.level_label"),
            items: vec![ToggleItem {
                id: "level_toggle".into(),
                label: toggle_label,
                selected: self.level == BackupLevel::Full,
                subtitle: Some(detail),
                a11y: Some(A11y {
                    label: Some(self.t("backup.wizard.level_a11y")),
                    hint: Some(self.t("backup.wizard.level_a11y_hint")),
                    role: Some(AccessibilityRole::Toggle),
                }),
                info_key: None,
            }],
            a11y: None,
        }
    }

    pub(super) fn choose_screen(&self) -> ScreenModel {
        let row = |id: &str, title: &str, detail: &str| ActionListItem {
            id: id.into(),
            label: self.t(title),
            icon: None,
            detail: Some(self.t(detail)),
            a11y: None,
            info_key: None,
        };
        let mut components = vec![
            Component::Text {
                id: "backup_intro".into(),
                content: self.t("backup.manage.intro"),
                style: TextStyle::Body,
                a11y: None,
            },
            Component::ActionList {
                id: "backup_rows".into(),
                items: vec![
                    row(
                        "create",
                        "backup.manage.export_title",
                        "backup.manage.export_detail",
                    ),
                    row(
                        "restore",
                        "backup.manage.restore_title",
                        "backup.manage.restore_detail",
                    ),
                ],
            },
            Component::Text {
                id: "last_backup".into(),
                content: self.last_backup_line(),
                style: TextStyle::Caption,
                a11y: None,
            },
        ];
        if let Some(on) = self.biometric_opt_in {
            components.push(self.biometric_toggle(on));
        }
        ScreenModel {
            screen_id: "backup_choose".into(),
            title: self.t("backup.wizard.title"),
            subtitle: None,
            components,
            contextual_actions: vec![],
            progress: None,
            ..Default::default()
        }
    }

    fn biometric_toggle(&self, on: bool) -> Component {
        let label = self.t("backup.biometric_toggle_label");
        Component::ToggleList {
            id: "biometric_export".into(),
            label: label.clone(),
            items: vec![ToggleItem {
                id: "biometric_toggle".into(),
                label: label.clone(),
                selected: on,
                subtitle: None,
                a11y: Some(A11y {
                    label: Some(label),
                    hint: None,
                    role: Some(AccessibilityRole::Toggle),
                }),
                info_key: None,
            }],
            a11y: None,
        }
    }

    pub(super) fn explain_biometric_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "backup_explain_biometric".into(),
            title: self.t("backup.biometric_explanation_title"),
            subtitle: None,
            components: vec![Component::Text {
                id: "biometric_explanation".into(),
                content: self.t("backup.biometric_explanation"),
                style: TextStyle::Body,
                a11y: None,
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: "back".into(),
                    label: self.t("action.back"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.back"))),
                },
                ScreenAction {
                    id: "turn_on".into(),
                    label: self.t("backup.biometric_toggle_label"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("backup.biometric_toggle_label"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    pub(super) fn password_screen(&self) -> ScreenModel {
        let label = match self.mode {
            BackupMode::Create => self.t("backup.wizard.choose_password"),
            BackupMode::Restore => self.t("backup.wizard.enter_password"),
        };
        let mut components = Vec::new();
        // Full or identity-only is an export question (#459).
        if matches!(self.mode, BackupMode::Create) {
            components.push(self.level_choice());
        }
        // Restore needs the backup blob. Offer a paste field (keyboard
        // frontends) ahead of the password; mobile may still drive restore
        // through the file picker, which sets `restore_data` out of band.
        if matches!(self.mode, BackupMode::Restore) {
            components.push(Component::TextInput {
                id: "backup_data".into(),
                label: self.t("backup.wizard.paste_label"),
                value: self.restore_data.clone(),
                placeholder: Some(self.t("backup.wizard.paste_placeholder")),
                max_length: None,
                validation_error: None,
                input_type: InputType::Text,
                a11y: Some(A11y {
                    label: Some(self.t("backup.wizard.paste_a11y")),
                    hint: Some(self.t("backup.wizard.paste_a11y_hint")),
                    role: Some(AccessibilityRole::TextField),
                }),
                info_key: None,
            });
        }
        // Reflect the captured value so keyboard frontends (which render the
        // model value rather than holding their own buffer) keep the input.
        components.push(Component::TextInput {
            id: "password".into(),
            label: label.clone(),
            value: self.password.clone(),
            placeholder: None,
            max_length: None,
            validation_error: None,
            input_type: InputType::Password,
            a11y: Some(A11y {
                label: Some(get_string_with_args(
                    self.locale,
                    "backup.wizard.password_a11y",
                    &[("label", &label)],
                )),
                hint: None,
                role: Some(AccessibilityRole::TextField),
            }),
            info_key: None,
        });
        let mut actions = vec![ScreenAction {
            id: "back".into(),
            label: self.t("action.back"),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: Some(A11y::labeled(self.t("action.back"))),
        }];
        // A real backup cannot arrive through the paste field: a 10k-contact
        // export is ~2.2 MB against a 4 KB MAX_EVENT_INPUT_VALUE_BYTES, so
        // the value is dropped and the screen dead-ends with Continue
        // disabled (2026-09-08-full-backup-restore-cannot-be-pasted).
        if matches!(self.mode, BackupMode::Restore) {
            actions.push(ScreenAction {
                id: "choose_file".into(),
                label: self.t("backup.wizard.choose_file"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("backup.wizard.choose_file"))),
            });
        }
        actions.push(ScreenAction {
            id: "continue".into(),
            label: self.t("action.continue"),
            style: ActionStyle::Primary,
            enabled: !self.password.is_empty(),
            a11y: Some(A11y::labeled(self.t("action.continue"))),
        });

        ScreenModel {
            screen_id: "backup_password".into(),
            title: self.t("backup.wizard.password_title"),
            subtitle: None,
            components,
            contextual_actions: actions,
            progress: self.progress(),
            ..Default::default()
        }
    }

    pub(super) fn restore_needs_empty_app_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "backup_restore_needs_empty".into(),
            title: self.t("backup.wizard.restore_needs_empty_title"),
            subtitle: None,
            components: vec![Component::Text {
                id: "restore_needs_empty".into(),
                content: self.t("backup.wizard.restore_needs_empty_body"),
                style: TextStyle::Body,
                a11y: None,
            }],
            contextual_actions: vec![ScreenAction {
                id: "back".into(),
                label: self.t("action.back"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.back"))),
            }],
            progress: self.progress(),
            ..Default::default()
        }
    }

    pub(super) fn confirm_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "backup_confirm".into(),
            title: self.t("backup.confirm_password"),
            subtitle: None,
            components: vec![Component::TextInput {
                id: "confirm_password".into(),
                label: self.t("backup.wizard.confirm_label"),
                value: self.confirm_password.clone(),
                placeholder: None,
                max_length: None,
                validation_error: None,
                input_type: InputType::Password,
                a11y: Some(A11y {
                    label: Some(get_string_with_args(
                        self.locale,
                        "backup.wizard.password_a11y",
                        &[("label", &self.t("backup.wizard.confirm_label"))],
                    )),
                    hint: None,
                    role: Some(AccessibilityRole::TextField),
                }),
                info_key: None,
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: "back".into(),
                    label: self.t("action.back"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.back"))),
                },
                ScreenAction {
                    id: "continue".into(),
                    label: self.t("action.continue"),
                    style: ActionStyle::Primary,
                    enabled: !self.confirm_password.is_empty(),
                    a11y: Some(A11y::labeled(self.t("action.continue"))),
                },
            ],
            progress: self.progress(),
            ..Default::default()
        }
    }

    pub(super) fn password_input(&self, id: &str, label: String, value: &str) -> Component {
        Component::TextInput {
            id: id.into(),
            label: label.clone(),
            value: value.to_string(),
            placeholder: None,
            max_length: None,
            validation_error: None,
            input_type: InputType::Password,
            a11y: Some(A11y {
                label: Some(get_string_with_args(
                    self.locale,
                    "backup.wizard.password_a11y",
                    &[("label", &label)],
                )),
                hint: None,
                role: Some(AccessibilityRole::TextField),
            }),
            info_key: None,
        }
    }

    pub(super) fn back_and_continue(&self, can_continue: bool) -> Vec<ScreenAction> {
        vec![
            ScreenAction {
                id: "back".into(),
                label: self.t("action.back"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.back"))),
            },
            ScreenAction {
                id: "continue".into(),
                label: self.t("action.continue"),
                style: ActionStyle::Primary,
                enabled: can_continue,
                a11y: Some(A11y::labeled(self.t("action.continue"))),
            },
        ]
    }

    pub(super) fn decoy_password_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "backup_decoy_password".into(),
            title: self.t("backup.wizard.decoy_password_title"),
            subtitle: None,
            components: vec![
                Component::Text {
                    id: "decoy_password_detail".into(),
                    content: self.t("backup.wizard.decoy_password_detail"),
                    style: TextStyle::Body,
                    a11y: None,
                },
                self.password_input(
                    "decoy_password",
                    self.t("backup.wizard.decoy_password_label"),
                    &self.decoy_password,
                ),
            ],
            contextual_actions: self.back_and_continue(!self.decoy_password.is_empty()),
            progress: self.progress(),
            ..Default::default()
        }
    }

    pub(super) fn decoy_confirm_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "backup_decoy_confirm".into(),
            title: self.t("backup.wizard.decoy_password_title"),
            subtitle: None,
            components: vec![self.password_input(
                "confirm_decoy_password",
                self.t("backup.wizard.decoy_confirm_label"),
                &self.confirm_decoy_password,
            )],
            contextual_actions: self.back_and_continue(!self.confirm_decoy_password.is_empty()),
            progress: self.progress(),
            ..Default::default()
        }
    }

    pub(super) fn processing_screen(&self) -> ScreenModel {
        let (title, detail) = match self.mode {
            BackupMode::Create => (
                self.t("backup.wizard.creating_title"),
                self.t("backup.wizard.creating_detail"),
            ),
            BackupMode::Restore => (
                self.t("backup.wizard.restoring_title"),
                self.t("backup.wizard.restoring_detail"),
            ),
        };
        ScreenModel {
            screen_id: "backup_processing".into(),
            title: title.clone(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "processing_status".into(),
                icon: None,
                title: title.clone(),
                detail: Some(detail.clone()),
                status: Status::InProgress,
                status_label: self.t(Status::InProgress.label_key()),
                a11y: Some(A11y {
                    label: Some(get_string_with_args(
                        self.locale,
                        "backup.wizard.status_a11y",
                        &[("title", &title)],
                    )),
                    hint: Some(detail),
                    role: None,
                }),
            }],
            contextual_actions: vec![],
            progress: self.progress(),
            ..Default::default()
        }
    }

    pub(super) fn complete_screen(&self) -> ScreenModel {
        let title = match self.mode {
            BackupMode::Create => self.t("backup.wizard.created_title"),
            BackupMode::Restore => self.t("backup.wizard.restored_title"),
        };
        ScreenModel {
            screen_id: "backup_complete".into(),
            title: title.clone(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "complete_status".into(),
                icon: None,
                title: title.clone(),
                detail: None,
                status: Status::Success,
                status_label: self.t(Status::Success.label_key()),
                a11y: Some(A11y {
                    label: Some(get_string_with_args(
                        self.locale,
                        "backup.wizard.status_a11y",
                        &[("title", &title)],
                    )),
                    hint: Some(self.t("backup.wizard.complete_hint")),
                    role: None,
                }),
            }],
            contextual_actions: vec![ScreenAction {
                id: "done".into(),
                label: self.t("action.done"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.done"))),
            }],
            progress: self.progress(),
            ..Default::default()
        }
    }

    pub(super) fn failed_screen(&self) -> ScreenModel {
        let title = match self.mode {
            BackupMode::Create => self.t("backup.wizard.create_failed_title"),
            BackupMode::Restore => self.t("backup.wizard.restore_failed_title"),
        };
        ScreenModel {
            screen_id: "backup_failed".into(),
            title: title.clone(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "failed_status".into(),
                icon: None,
                title: title.clone(),
                detail: None,
                status: Status::Failed,
                status_label: self.t(Status::Failed.label_key()),
                a11y: Some(A11y {
                    label: Some(get_string_with_args(
                        self.locale,
                        "backup.wizard.status_a11y",
                        &[("title", &title)],
                    )),
                    hint: Some(self.t("backup.wizard.failed_hint")),
                    role: None,
                }),
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: "retry".into(),
                    label: self.t("action.retry"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.retry"))),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: self.progress(),
            ..Default::default()
        }
    }
}
