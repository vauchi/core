// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The opt-in OS check in front of a backup export (ADR-080 point 7,
//! amended 2026-11-09): it confirms only that someone is at the phone, and
//! the session PIN always follows it. Under duress the mask holds no
//! user-presence enrolment, so any successful check passes there, and the
//! flow looks the same as on the real profile.

use super::credential_gate::CredentialChange;
use super::{AppEngine, AppScreen};
use crate::ui::{ActionResult, EngineOutput};
use vauchi_core::{AuthMode, Event};

const USER_PRESENCE_TRANSPORT: &str = "user_presence";

impl AppEngine {
    /// Starts a confirmed backup export: the OS check first when the user
    /// opted in on a device with biometrics, then the session PIN.
    pub(super) fn confirm_backup_export(&mut self) -> ActionResult {
        let opted_in = self
            .vauchi
            .confirms_exports_with_biometrics()
            .unwrap_or(false);
        if opted_in && self.device_has_biometrics() {
            self.pending_backup_presence = true;
            self.request_user_presence("backup.export_presence_prompt");
            return ActionResult::UpdateScreen(self.engine.current_screen());
        }
        self.confirm_session_pin_then(CredentialChange::BackupExport)
    }

    /// The shell's answer to the OS check a backup export is waiting on.
    pub(super) fn handle_backup_presence(&mut self, event: &Event) -> Option<ActionResult> {
        let on_mask = self.vauchi.auth_mode() == AuthMode::Duress;
        let confirmed = match event {
            Event::UserPresenceSecretCreated { secret } => {
                on_mask
                    || self
                        .vauchi
                        .enroll_user_presence_secret(secret.as_bytes())
                        .unwrap_or(false)
            }
            Event::UserPresenceSecretRevealed { secret } => {
                on_mask
                    || self
                        .vauchi
                        .confirm_user_presence_secret(secret.as_bytes())
                        .unwrap_or(false)
            }
            Event::UserPresenceCheckCancelled | Event::UserPresenceSecretMissing => false,
            Event::HardwareUnavailable { transport } if transport == USER_PRESENCE_TRANSPORT => {
                false
            }
            _ => return None,
        };
        self.pending_backup_presence = false;
        if confirmed {
            return Some(self.confirm_session_pin_then(CredentialChange::BackupExport));
        }
        self.engine.processing_failed();
        Some(ActionResult::NavigateTo(self.engine.current_screen()))
    }

    /// Saves the Backup screen's opt-in when it changed there; the core
    /// mirrors it to the mask.
    pub(super) fn persist_backup_biometric_opt_in(&mut self) {
        if !matches!(self.screen, AppScreen::Backup) {
            return;
        }
        let Some(EngineOutput::Backup(snap)) = self.engine.engine_output() else {
            return;
        };
        let Some(wanted) = snap.biometric_opt_in else {
            return;
        };
        if self.vauchi.confirms_exports_with_biometrics().ok() != Some(wanted)
            && let Err(error) = self.vauchi.set_confirm_exports_with_biometrics(wanted)
        {
            tracing::warn!(%error, "[Backup] Failed: saving the export biometric opt-in");
        }
    }
}
