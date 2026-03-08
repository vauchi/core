// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Backup & recovery workflow engine.

use crate::ui::*;

/// Whether the user is creating or restoring a backup.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BackupMode {
    Create,
    Restore,
}

/// Backup & recovery workflow engine.
pub struct BackupRecoveryEngine {
    step: BackupStep,
    mode: BackupMode,
    password: String,
    confirm_password: String,
}

#[derive(Clone, Debug, PartialEq)]
enum BackupStep {
    ChooseMode,
    EnterPassword,
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
    pub fn new(mode: Option<BackupMode>) -> Self {
        let (step, mode) = match mode {
            Some(m) => (BackupStep::EnterPassword, m),
            None => (BackupStep::ChooseMode, BackupMode::Create),
        };
        Self {
            step,
            mode,
            password: String::new(),
            confirm_password: String::new(),
        }
    }

    /// Signals that async processing completed successfully.
    pub fn processing_complete(&mut self) {
        self.step = BackupStep::Complete;
    }

    /// Signals that async processing failed.
    pub fn processing_failed(&mut self) {
        self.step = BackupStep::Failed;
    }

    fn total_steps(&self) -> u8 {
        match self.mode {
            BackupMode::Create => 4,
            BackupMode::Restore => 3,
        }
    }

    fn progress(&self) -> Option<Progress> {
        let current_step = match self.step {
            BackupStep::ChooseMode => return None,
            BackupStep::EnterPassword => 1,
            BackupStep::ConfirmPassword => 2,
            BackupStep::Processing => match self.mode {
                BackupMode::Create => 3,
                BackupMode::Restore => 2,
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
        ScreenModel {
            screen_id: "backup_choose".into(),
            title: "Backup & Recovery".into(),
            subtitle: None,
            components: vec![Component::InfoPanel {
                id: "backup_info".into(),
                icon: Some("backup".into()),
                title: "Protect your data".into(),
                items: vec![InfoItem {
                    icon: None,
                    title: "Backup".into(),
                    detail: "Create an encrypted backup of your identity and contacts.".into(),
                }],
            }],
            actions: vec![
                ScreenAction {
                    id: "create".into(),
                    label: "Create Backup".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "restore".into(),
                    label: "Restore Backup".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: None,
        }
    }

    fn password_screen(&self) -> ScreenModel {
        let label = match self.mode {
            BackupMode::Create => "Choose a backup password",
            BackupMode::Restore => "Enter your backup password",
        };
        ScreenModel {
            screen_id: "backup_password".into(),
            title: "Backup Password".into(),
            subtitle: None,
            components: vec![Component::TextInput {
                id: "password".into(),
                label: label.into(),
                value: self.password.clone(),
                placeholder: None,
                max_length: None,
                validation_error: None,
                input_type: InputType::Text,
            }],
            actions: vec![
                ScreenAction {
                    id: "back".into(),
                    label: "Back".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
                ScreenAction {
                    id: "continue".into(),
                    label: "Continue".into(),
                    style: ActionStyle::Primary,
                    enabled: !self.password.is_empty(),
                },
            ],
            progress: self.progress(),
        }
    }

    fn confirm_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "backup_confirm".into(),
            title: "Confirm Password".into(),
            subtitle: None,
            components: vec![Component::TextInput {
                id: "confirm_password".into(),
                label: "Confirm your backup password".into(),
                value: self.confirm_password.clone(),
                placeholder: None,
                max_length: None,
                validation_error: None,
                input_type: InputType::Text,
            }],
            actions: vec![
                ScreenAction {
                    id: "back".into(),
                    label: "Back".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
                ScreenAction {
                    id: "continue".into(),
                    label: "Continue".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
            ],
            progress: self.progress(),
        }
    }

    fn processing_screen(&self) -> ScreenModel {
        let title = match self.mode {
            BackupMode::Create => "Creating backup…",
            BackupMode::Restore => "Restoring backup…",
        };
        ScreenModel {
            screen_id: "backup_processing".into(),
            title: title.into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "processing_status".into(),
                icon: None,
                title: title.into(),
                detail: None,
                status: Status::InProgress,
            }],
            actions: vec![],
            progress: self.progress(),
        }
    }

    fn complete_screen(&self) -> ScreenModel {
        let title = match self.mode {
            BackupMode::Create => "Backup Created",
            BackupMode::Restore => "Backup Restored",
        };
        ScreenModel {
            screen_id: "backup_complete".into(),
            title: title.into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "complete_status".into(),
                icon: None,
                title: title.into(),
                detail: None,
                status: Status::Success,
            }],
            actions: vec![ScreenAction {
                id: "done".into(),
                label: "Done".into(),
                style: ActionStyle::Primary,
                enabled: true,
            }],
            progress: self.progress(),
        }
    }

    fn failed_screen(&self) -> ScreenModel {
        let title = match self.mode {
            BackupMode::Create => "Backup Failed",
            BackupMode::Restore => "Restore Failed",
        };
        ScreenModel {
            screen_id: "backup_failed".into(),
            title: title.into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "failed_status".into(),
                icon: None,
                title: title.into(),
                detail: None,
                status: Status::Failed,
            }],
            actions: vec![
                ScreenAction {
                    id: "retry".into(),
                    label: "Retry".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: self.progress(),
        }
    }
}

impl WorkflowEngine for BackupRecoveryEngine {
    fn current_screen(&self) -> ScreenModel {
        match self.step {
            BackupStep::ChooseMode => self.choose_screen(),
            BackupStep::EnterPassword => self.password_screen(),
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
            (BackupStep::EnterPassword, UserAction::ActionPressed { action_id })
                if action_id == "continue" =>
            {
                if self.password.is_empty() {
                    return ActionResult::ValidationError {
                        component_id: "password".into(),
                        message: "Password is required".into(),
                    };
                }
                match self.mode {
                    BackupMode::Create => {
                        self.step = BackupStep::ConfirmPassword;
                    }
                    BackupMode::Restore => {
                        self.step = BackupStep::Processing;
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
                if action_id == "continue" =>
            {
                if self.confirm_password != self.password {
                    return ActionResult::ValidationError {
                        component_id: "confirm_password".into(),
                        message: "Passwords do not match".into(),
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
}
