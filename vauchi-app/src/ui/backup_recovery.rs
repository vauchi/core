// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Backup & recovery workflow engine.
//! Copy resolves through `i18n::get_string` in the locale threaded at
//! construction (M3 S3d of `2026-07-03-core-screens-bypass-i18n`); keys
//! live in the `backup.wizard.*` + shared `backup.*` / `action.*` families.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;
use zeroize::Zeroize;

/// Whether the user is creating or restoring a backup.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum BackupMode {
    Create,
    Restore,
}

/// What data the backup includes.
///
/// Option D decision: full is the default, identity-only is opt-in.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum BackupLevel {
    /// Identity + contacts + own card + labels (v3 format).
    #[default]
    Full,
    /// Identity only — master seed + device info (v2 format).
    IdentityOnly,
}

/// Backup & recovery workflow engine.
pub struct BackupRecoveryEngine {
    step: BackupStep,
    mode: BackupMode,
    level: BackupLevel,
    password: String,
    confirm_password: String,
    has_identity: bool,
    /// Pasted backup blob (Restore mode only). Hex-encoded ASCII matching
    /// `Vauchi::export_full_backup` output. Captured on the password screen.
    restore_data: String,
    locale: Locale,
}

impl Drop for BackupRecoveryEngine {
    fn drop(&mut self) {
        self.password.zeroize();
        self.confirm_password.zeroize();
        self.restore_data.zeroize();
    }
}

#[derive(Clone, Debug, PartialEq)]
enum BackupStep {
    ChooseMode,
    EnterPassword,
    ConfirmReplace,
    ConfirmPassword,
    Processing,
    Complete,
    Failed,
}

impl BackupRecoveryEngine {
    /// Creates a new backup/recovery engine.
    ///
    /// If `mode` is `Some`, starts at the password entry step.
    /// If `None`, starts at the mode selection step.
    pub fn new(mode: Option<BackupMode>, has_identity: bool, locale: Locale) -> Self {
        let (step, mode) = match mode {
            Some(m) => (BackupStep::EnterPassword, m),
            None => (BackupStep::ChooseMode, BackupMode::Create),
        };
        Self {
            step,
            mode,
            level: BackupLevel::Full,
            password: String::new(),
            confirm_password: String::new(),
            has_identity,
            restore_data: String::new(),
            locale,
        }
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Returns the pasted backup blob (Restore mode). Only meaningful once
    /// the user has pasted it on the password screen; consumed by the
    /// AppEngine at `Processing` to drive `Vauchi::import_full_backup`.
    pub fn restore_data(&self) -> &str {
        &self.restore_data
    }

    /// Returns the current backup level (full or identity-only).
    ///
    /// Callers check this when the engine reaches `Processing` to decide
    /// whether to call `export_full_backup()` or `export_backup()`.
    pub fn level(&self) -> &BackupLevel {
        &self.level
    }

    /// Returns the current mode (create or restore).
    pub fn mode(&self) -> &BackupMode {
        &self.mode
    }

    /// Returns the password entered by the user.
    ///
    /// Only meaningful when the engine is in `Processing` state.
    pub fn password(&self) -> &str {
        &self.password
    }

    fn total_steps(&self) -> u8 {
        match self.mode {
            BackupMode::Create => 4,
            BackupMode::Restore => {
                if self.has_identity {
                    4
                } else {
                    3
                }
            }
        }
    }

    fn progress(&self) -> Option<Progress> {
        let current_step = match self.step {
            BackupStep::ChooseMode => return None,
            BackupStep::EnterPassword => 1,
            BackupStep::ConfirmReplace => 2,
            BackupStep::ConfirmPassword => 2,
            BackupStep::Processing => match self.mode {
                BackupMode::Create => 3,
                BackupMode::Restore => {
                    if self.has_identity {
                        3
                    } else {
                        2
                    }
                }
            },
            BackupStep::Complete => self.total_steps(),
            BackupStep::Failed => self.total_steps(),
        };
        Some(Progress {
            current_step,
            total_steps: self.total_steps(),
            label: None,
        })
    }

    fn choose_screen(&self) -> ScreenModel {
        let detail = match self.level {
            BackupLevel::Full => self.t("backup.wizard.full_detail"),
            BackupLevel::IdentityOnly => self.t("backup.wizard.identity_only_detail"),
        };
        let toggle_label = match self.level {
            BackupLevel::Full => self.t("backup.wizard.level_full"),
            BackupLevel::IdentityOnly => self.t("backup.wizard.level_identity_only"),
        };
        ScreenModel {
            screen_id: "backup_choose".into(),
            title: self.t("backup.wizard.title"),
            subtitle: None,
            components: vec![
                Component::InfoPanel {
                    id: "backup_info".into(),
                    icon: Some("drive".into()),
                    title: self.t("backup.wizard.protect_title"),
                    items: vec![InfoItem {
                        icon: None,
                        title: self.t("backup.wizard.backup_item"),
                        detail,
                    }],
                    a11y: None,
                },
                Component::ToggleList {
                    id: "backup_level".into(),
                    label: self.t("backup.wizard.level_label"),
                    items: vec![ToggleItem {
                        id: "level_toggle".into(),
                        label: toggle_label,
                        selected: self.level == BackupLevel::Full,
                        subtitle: None,
                        a11y: Some(A11y {
                            label: Some(self.t("backup.wizard.level_a11y")),
                            hint: Some(self.t("backup.wizard.level_a11y_hint")),
                            role: Some(AccessibilityRole::Toggle),
                        }),
                        info_key: None,
                    }],
                    a11y: None,
                },
            ],
            actions: vec![
                ScreenAction {
                    id: "create".into(),
                    label: self.t("backup.wizard.create"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("backup.wizard.create"))),
                },
                ScreenAction {
                    id: "restore".into(),
                    label: self.t("backup.wizard.restore"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("backup.wizard.restore"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn password_screen(&self) -> ScreenModel {
        let label = match self.mode {
            BackupMode::Create => self.t("backup.wizard.choose_password"),
            BackupMode::Restore => self.t("backup.wizard.enter_password"),
        };
        let mut components = Vec::new();
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
        ScreenModel {
            screen_id: "backup_password".into(),
            title: self.t("backup.wizard.password_title"),
            subtitle: None,
            components,
            actions: vec![
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
                    enabled: !self.password.is_empty(),
                    a11y: Some(A11y::labeled(self.t("action.continue"))),
                },
            ],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn confirm_replace_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "backup_confirm_replace".into(),
            title: self.t("backup.replace_confirm"),
            subtitle: None,
            components: vec![Component::InlineConfirm {
                id: "replace".into(),
                warning: self.t("backup.wizard.replace_warning"),
                confirm_text: self.t("backup.replace_button"),
                cancel_text: self.t("action.cancel"),
                confirm_action_id: "confirm_replace".into(),
                cancel_action_id: "cancel_replace".into(),
                destructive: true,
                a11y: Some(A11y {
                    label: Some(self.t("backup.wizard.replace_a11y")),
                    hint: Some(self.t("backup.wizard.replace_warning")),
                    role: Some(AccessibilityRole::Alert),
                }),
            }],
            actions: vec![],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn confirm_screen(&self) -> ScreenModel {
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
            actions: vec![
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

    fn processing_screen(&self) -> ScreenModel {
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
            actions: vec![],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn complete_screen(&self) -> ScreenModel {
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
            actions: vec![ScreenAction {
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

    fn failed_screen(&self) -> ScreenModel {
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
            actions: vec![
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

impl WorkflowEngine for BackupRecoveryEngine {
    fn engine_output(&self) -> Option<EngineOutput> {
        Some(EngineOutput::Backup(BackupFormSnapshot {
            restore_mode: *self.mode() == BackupMode::Restore,
            restore_data: self.restore_data().trim().to_string(),
            password: self.password().to_string(),
            full_level: *self.level() == BackupLevel::Full,
        }))
    }

    fn current_screen(&self) -> ScreenModel {
        match self.step {
            BackupStep::ChooseMode => self.choose_screen(),
            BackupStep::EnterPassword => self.password_screen(),
            BackupStep::ConfirmReplace => self.confirm_replace_screen(),
            BackupStep::ConfirmPassword => self.confirm_screen(),
            BackupStep::Processing => self.processing_screen(),
            BackupStep::Complete => self.complete_screen(),
            BackupStep::Failed => self.failed_screen(),
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match (&self.step, action) {
            // ChooseMode
            (BackupStep::ChooseMode, UserAction::ActionPressed { action_id })
                if action_id == "create" =>
            {
                self.mode = BackupMode::Create;
                self.step = BackupStep::EnterPassword;
                ActionResult::NavigateTo(self.current_screen())
            }
            (BackupStep::ChooseMode, UserAction::ActionPressed { action_id })
                if action_id == "restore" =>
            {
                self.mode = BackupMode::Restore;
                self.step = BackupStep::EnterPassword;
                ActionResult::NavigateTo(self.current_screen())
            }
            (
                BackupStep::ChooseMode,
                UserAction::ItemToggled {
                    component_id,
                    item_id,
                },
            ) if component_id == "backup_level" && item_id == "level_toggle" => {
                self.level = match self.level {
                    BackupLevel::Full => BackupLevel::IdentityOnly,
                    BackupLevel::IdentityOnly => BackupLevel::Full,
                };
                ActionResult::UpdateScreen(self.current_screen())
            }

            // EnterPassword
            (
                BackupStep::EnterPassword,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "password" => {
                self.password = value;
                ActionResult::UpdateScreen(self.current_screen())
            }
            (
                BackupStep::EnterPassword,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "backup_data" => {
                self.restore_data = value;
                ActionResult::UpdateScreen(self.current_screen())
            }
            (BackupStep::EnterPassword, UserAction::ActionPressed { action_id })
                if action_id == "continue"
                    || action_id == "submit_password"
                    || action_id == "submit_backup_data" =>
            {
                if self.password.is_empty() {
                    return ActionResult::ValidationError {
                        component_id: "password".into(),
                        message: self.t("backup.error_enter_password"),
                    };
                }
                if matches!(self.mode, BackupMode::Restore) && self.restore_data.trim().is_empty() {
                    return ActionResult::ValidationError {
                        component_id: "backup_data".into(),
                        message: self.t("backup.error_paste_data"),
                    };
                }
                match self.mode {
                    BackupMode::Create => {
                        self.step = BackupStep::ConfirmPassword;
                    }
                    BackupMode::Restore => {
                        if self.has_identity {
                            self.step = BackupStep::ConfirmReplace;
                        } else {
                            self.step = BackupStep::Processing;
                        }
                    }
                }
                ActionResult::NavigateTo(self.current_screen())
            }
            (BackupStep::EnterPassword, UserAction::ActionPressed { action_id })
                if action_id == "back" =>
            {
                self.step = BackupStep::ChooseMode;
                ActionResult::NavigateTo(self.current_screen())
            }

            // ConfirmReplace
            (BackupStep::ConfirmReplace, UserAction::ActionPressed { action_id })
                if action_id == "confirm_replace" =>
            {
                self.step = BackupStep::Processing;
                ActionResult::NavigateTo(self.current_screen())
            }
            (BackupStep::ConfirmReplace, UserAction::ActionPressed { action_id })
                if action_id == "cancel_replace" =>
            {
                self.step = BackupStep::EnterPassword;
                ActionResult::NavigateTo(self.current_screen())
            }

            // ConfirmPassword
            (
                BackupStep::ConfirmPassword,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "confirm_password" => {
                self.confirm_password = value;
                ActionResult::UpdateScreen(self.current_screen())
            }
            (BackupStep::ConfirmPassword, UserAction::ActionPressed { action_id })
                if action_id == "continue" || action_id == "submit_confirm_password" =>
            {
                if self.confirm_password != self.password {
                    return ActionResult::ValidationError {
                        component_id: "confirm_password".into(),
                        message: self.t("backup.error_passwords_mismatch"),
                    };
                }
                self.step = BackupStep::Processing;
                ActionResult::NavigateTo(self.current_screen())
            }
            (BackupStep::ConfirmPassword, UserAction::ActionPressed { action_id })
                if action_id == "back" =>
            {
                self.step = BackupStep::EnterPassword;
                ActionResult::NavigateTo(self.current_screen())
            }

            // Complete
            (BackupStep::Complete, UserAction::ActionPressed { action_id })
                if action_id == "done" =>
            {
                ActionResult::Complete
            }

            // Failed
            (BackupStep::Failed, UserAction::ActionPressed { action_id })
                if action_id == "retry" =>
            {
                self.password.zeroize();
                self.confirm_password.zeroize();
                self.step = BackupStep::EnterPassword;
                ActionResult::NavigateTo(self.current_screen())
            }
            (BackupStep::Failed, UserAction::ActionPressed { action_id })
                if action_id == "cancel" =>
            {
                ActionResult::Complete
            }

            // Default: refresh current screen
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn processing_complete(&mut self) {
        if self.step == BackupStep::Processing {
            self.step = BackupStep::Complete;
        }
    }

    fn processing_failed(&mut self) {
        if self.step == BackupStep::Processing {
            self.step = BackupStep::Failed;
        }
    }
}
