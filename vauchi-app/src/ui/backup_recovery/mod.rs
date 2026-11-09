// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Backup & recovery workflow engine.
//! Copy resolves through `i18n::get_string` in the locale threaded at
//! construction (M3 S3d of `2026-07-03-core-screens-bypass-i18n`); keys
//! live in the `backup.wizard.*` + shared `backup.*` / `action.*` families.

mod screens;

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;
use vauchi_core::{Command, FilePickPurpose};
use zeroize::Zeroize;

/// Backup exports are hex-encoded ASCII; pickers vary in what they report,
/// so accept the plain-text families rather than a single strict type.
fn backup_mime_types() -> Vec<String> {
    vec!["text/plain".to_string(), "*/*".to_string()]
}

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
    /// Whether this export asks for a decoy password: the real profile
    /// with a duress PIN set (ADR-080 point 7, amended 2026-11-09).
    offers_decoy: bool,
    decoy_password: String,
    confirm_decoy_password: String,
    has_identity: bool,
    /// Pasted backup blob (Restore mode only). Hex-encoded ASCII matching
    /// `Vauchi::export_full_backup` output. Captured on the password screen.
    restore_data: String,
    last_backup: Option<u64>,
    now: u64,
    locale: Locale,
}

impl Drop for BackupRecoveryEngine {
    fn drop(&mut self) {
        self.password.zeroize();
        self.confirm_password.zeroize();
        self.decoy_password.zeroize();
        self.confirm_decoy_password.zeroize();
        self.restore_data.zeroize();
    }
}

#[derive(Clone, Debug, PartialEq)]
enum BackupStep {
    ChooseMode,
    EnterPassword,
    /// A backup restores only into an app with no identity (vauchi/private#819).
    RestoreNeedsEmptyApp,
    ConfirmPassword,
    /// The decoy password of a full backup from the real profile with a
    /// duress PIN set (ADR-080 point 7).
    EnterDecoyPassword,
    ConfirmDecoyPassword,
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
            Some(m) => (Self::first_step(&m, has_identity), m),
            None => (BackupStep::ChooseMode, BackupMode::Create),
        };
        Self {
            step,
            mode,
            level: BackupLevel::Full,
            password: String::new(),
            confirm_password: String::new(),
            offers_decoy: false,
            decoy_password: String::new(),
            confirm_decoy_password: String::new(),
            has_identity,
            restore_data: String::new(),
            last_backup: None,
            now: 0,
            locale,
        }
    }

    /// Whether a full export asks for a decoy password: set for the real
    /// profile with a duress PIN, never under duress.
    pub fn with_decoy_offer(mut self, offers_decoy: bool) -> Self {
        self.offers_decoy = offers_decoy;
        self
    }

    fn asks_for_decoy(&self) -> bool {
        self.offers_decoy
            && matches!(self.mode, BackupMode::Create)
            && matches!(self.level, BackupLevel::Full)
    }

    /// When the last backup was made (Unix seconds), and the current time
    /// to say how long ago that was.
    pub fn with_last_backup(mut self, last_backup: Option<u64>, now: u64) -> Self {
        self.last_backup = last_backup;
        self.now = now;
        self
    }

    fn first_step(mode: &BackupMode, has_identity: bool) -> BackupStep {
        match mode {
            BackupMode::Restore if has_identity => BackupStep::RestoreNeedsEmptyApp,
            _ => BackupStep::EnterPassword,
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
            BackupMode::Create if self.asks_for_decoy() => 6,
            BackupMode::Create => 4,
            BackupMode::Restore => 3,
        }
    }

    fn progress(&self) -> Option<Progress> {
        let current_step = match self.step {
            BackupStep::ChooseMode | BackupStep::RestoreNeedsEmptyApp => return None,
            BackupStep::EnterPassword => 1,
            BackupStep::ConfirmPassword => 2,
            BackupStep::EnterDecoyPassword => 3,
            BackupStep::ConfirmDecoyPassword => 4,
            BackupStep::Processing => match self.mode {
                BackupMode::Create if self.asks_for_decoy() => 5,
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
}

impl WorkflowEngine for BackupRecoveryEngine {
    fn engine_output(&self) -> Option<EngineOutput> {
        Some(EngineOutput::Backup(BackupFormSnapshot {
            restore_mode: *self.mode() == BackupMode::Restore,
            restore_data: self.restore_data().trim().to_string(),
            password: self.password().to_string(),
            full_level: *self.level() == BackupLevel::Full,
            decoy_password: if self.asks_for_decoy() {
                self.decoy_password.clone()
            } else {
                String::new()
            },
        }))
    }

    fn current_screen(&self) -> ScreenModel {
        match self.step {
            BackupStep::ChooseMode => self.choose_screen(),
            BackupStep::EnterPassword => self.password_screen(),
            BackupStep::RestoreNeedsEmptyApp => self.restore_needs_empty_app_screen(),
            BackupStep::ConfirmPassword => self.confirm_screen(),
            BackupStep::EnterDecoyPassword => self.decoy_password_screen(),
            BackupStep::ConfirmDecoyPassword => self.decoy_confirm_screen(),
            BackupStep::Processing => self.processing_screen(),
            BackupStep::Complete => self.complete_screen(),
            BackupStep::Failed => self.failed_screen(),
        }
    }

    fn apply_update(&mut self, update: crate::ui::EngineUpdate) -> bool {
        let crate::ui::EngineUpdate::BackupRecovery(update) = update else {
            return false;
        };
        match update {
            crate::ui::BackupRecoveryUpdate::PickedBackupBytes(_) if self.has_identity => {
                return false;
            }
            crate::ui::BackupRecoveryUpdate::PickedBackupBytes(bytes) => {
                // The picker hands back the exported file verbatim; the
                // export is hex-encoded ASCII, so lossy decoding cannot
                // corrupt a valid backup and a binary file simply fails
                // to decrypt with a clear error rather than panicking.
                self.restore_data = String::from_utf8_lossy(&bytes).into_owned();
                self.mode = BackupMode::Restore;
                self.step = BackupStep::EnterPassword;
            }
        }
        true
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match (&self.step, action) {
            // ChooseMode
            (
                BackupStep::ChooseMode,
                UserAction::ListItemSelected {
                    component_id,
                    item_id,
                },
            ) if component_id == "backup_rows" && (item_id == "create" || item_id == "restore") => {
                self.mode = if item_id == "create" {
                    BackupMode::Create
                } else {
                    BackupMode::Restore
                };
                self.step = Self::first_step(&self.mode, self.has_identity);
                ActionResult::NavigateTo(self.current_screen())
            }
            (
                BackupStep::EnterPassword,
                UserAction::ItemToggled {
                    component_id,
                    item_id,
                },
            ) if self.mode == BackupMode::Create
                && component_id == "backup_level"
                && item_id == "level_toggle" =>
            {
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
                if action_id == "choose_file" =>
            {
                ActionResult::Commands {
                    commands: vec![Command::FilePickFromUser {
                        max_bytes: Some(vauchi_core::MAX_FILE_PICK_BYTES as u64),
                        accepted_mime_types: backup_mime_types(),
                        accepted_extensions: FilePickPurpose::ImportBackup.accepted_extensions(),
                        purpose: FilePickPurpose::ImportBackup,
                    }],
                }
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

            (BackupStep::RestoreNeedsEmptyApp, UserAction::ActionPressed { action_id })
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
                if action_id == "continue" || action_id == "submit_confirm_password" =>
            {
                if self.confirm_password != self.password {
                    return ActionResult::ValidationError {
                        component_id: "confirm_password".into(),
                        message: self.t("backup.error_passwords_mismatch"),
                    };
                }
                self.step = if self.asks_for_decoy() {
                    BackupStep::EnterDecoyPassword
                } else {
                    BackupStep::Processing
                };
                ActionResult::NavigateTo(self.current_screen())
            }

            // EnterDecoyPassword
            (
                BackupStep::EnterDecoyPassword,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "decoy_password" => {
                self.decoy_password = value;
                ActionResult::UpdateScreen(self.current_screen())
            }
            (BackupStep::EnterDecoyPassword, UserAction::ActionPressed { action_id })
                if action_id == "continue" =>
            {
                if self.decoy_password.is_empty() {
                    return ActionResult::ValidationError {
                        component_id: "decoy_password".into(),
                        message: self.t("backup.error_enter_password"),
                    };
                }
                self.step = BackupStep::ConfirmDecoyPassword;
                ActionResult::NavigateTo(self.current_screen())
            }
            (BackupStep::EnterDecoyPassword, UserAction::ActionPressed { action_id })
                if action_id == "back" =>
            {
                self.step = BackupStep::ConfirmPassword;
                ActionResult::NavigateTo(self.current_screen())
            }

            // ConfirmDecoyPassword
            (
                BackupStep::ConfirmDecoyPassword,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "confirm_decoy_password" => {
                self.confirm_decoy_password = value;
                ActionResult::UpdateScreen(self.current_screen())
            }
            (BackupStep::ConfirmDecoyPassword, UserAction::ActionPressed { action_id })
                if action_id == "continue" =>
            {
                if self.confirm_decoy_password != self.decoy_password {
                    return ActionResult::ValidationError {
                        component_id: "confirm_decoy_password".into(),
                        message: self.t("backup.error_passwords_mismatch"),
                    };
                }
                self.step = BackupStep::Processing;
                ActionResult::NavigateTo(self.current_screen())
            }
            (BackupStep::ConfirmDecoyPassword, UserAction::ActionPressed { action_id })
                if action_id == "back" =>
            {
                self.step = BackupStep::EnterDecoyPassword;
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
                self.decoy_password.zeroize();
                self.confirm_decoy_password.zeroize();
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
