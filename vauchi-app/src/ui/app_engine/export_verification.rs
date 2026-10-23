// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The PIN step in front of every data export (#712): check the PIN against
//! the one that opened this session, run the pending export once on a
//! match, and count misses with the other PIN re-confirmation steps.

use super::{AppEngine, AppScreen};
use crate::ui::device_link_pin::CONFIRM_PIN_ACTION_ID;
use crate::ui::gdpr::SET_APP_PASSWORD_ACTION_ID;
use crate::ui::lock_screen::DEFAULT_LOCK_MAX_ATTEMPTS;
use crate::ui::{
    ActionResult, EngineOutput, EngineUpdate, GdprChoice, GdprExportUpdate, UserAction,
};
use vauchi_core::{Command, Event};
use zeroize::Zeroize;

/// The `transport` a shell names in `HardwareUnavailable` when it cannot
/// run the user-presence check.
const USER_PRESENCE_TRANSPORT: &str = "user_presence";

impl AppEngine {
    pub(super) fn intercept_export_pin(&mut self, action: &UserAction) -> Option<ActionResult> {
        let UserAction::ActionPressed { action_id } = action else {
            return None;
        };
        if !matches!(self.screen, AppScreen::Privacy) {
            return None;
        }
        if action_id == SET_APP_PASSWORD_ACTION_ID {
            let screen = self.navigate_to(AppScreen::ChangePassword);
            return Some(ActionResult::NavigateTo(screen));
        }
        if action_id == "export" || action_id == "confirm_heirloom" {
            let result = self.engine.handle_action(action.clone());
            if matches!(
                self.engine.engine_output(),
                Some(EngineOutput::AwaitingUserPresence)
            ) {
                self.request_user_presence("privacy.export_presence_prompt");
            }
            return Some(result);
        }
        if action_id != CONFIRM_PIN_ACTION_ID {
            return None;
        }
        // An empty entry stays with the engine, which keeps the step.
        let Some(EngineOutput::ExportPin { mut pin }) = self.engine.engine_output() else {
            return None;
        };
        let confirmed = self.vauchi.confirm_session_password(&pin).unwrap_or(false);
        pin.zeroize();
        if confirmed {
            self.confirm_pin_failures = 0;
            // The password proved the user, so an enrolment whose sealed
            // secret may be gone (#739) can go; the OS check, once the
            // password is removed, seals a fresh one.
            if self.vauchi.forget_user_presence_secret().is_err() {
                tracing::warn!("user-presence enrolment not cleared");
            }
            let _ = self
                .engine
                .apply_update(EngineUpdate::GdprExport(GdprExportUpdate::Verified));
            // The heirloom exports now; the data export first asks for a format.
            if matches!(self.engine.engine_output(), Some(EngineOutput::Gdpr(_))) {
                return Some(self.complete_privacy());
            }
            return Some(ActionResult::NavigateTo(self.engine.current_screen()));
        }
        if let Some(locked) = self.record_confirm_pin_miss() {
            return Some(locked);
        }
        let _ = self
            .engine
            .apply_update(EngineUpdate::GdprExport(GdprExportUpdate::PinRejected));
        Some(ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Asks the shell for the OS user-presence check: to seal a fresh secret
    /// the first time on this device, to reveal it after that.
    pub(super) fn request_user_presence(&mut self, prompt_key: &str) {
        let prompt = self.t(prompt_key);
        let command = if self.vauchi.has_user_presence_secret().unwrap_or(false) {
            Command::RevealUserPresenceSecret { prompt }
        } else {
            Command::CreateUserPresenceSecret { prompt }
        };
        self.pending_commands.push_back(command);
    }

    /// The shell's answer to the user-presence check. `None` unless an
    /// export is waiting on it: a secret nobody asked for confirms nothing.
    pub(super) fn handle_user_presence_event(&mut self, event: &Event) -> Option<ActionResult> {
        if self.pending_first_password.is_some() {
            return self.handle_first_password_presence(event);
        }
        if !matches!(self.screen, AppScreen::Privacy)
            || !matches!(
                self.engine.engine_output(),
                Some(EngineOutput::AwaitingUserPresence)
            )
        {
            return None;
        }
        let confirmed = match event {
            Event::UserPresenceSecretCreated { secret } => self
                .vauchi
                .enroll_user_presence_secret(secret.as_bytes())
                .unwrap_or(false),
            Event::UserPresenceSecretRevealed { secret } => self
                .vauchi
                .confirm_user_presence_secret(secret.as_bytes())
                .unwrap_or(false),
            Event::UserPresenceCheckCancelled => {
                let _ = self
                    .engine
                    .apply_update(EngineUpdate::GdprExport(GdprExportUpdate::Finished));
                return Some(ActionResult::NavigateTo(self.engine.current_screen()));
            }
            Event::HardwareUnavailable { transport } if transport == USER_PRESENCE_TRANSPORT => {
                let _ = self.engine.apply_update(EngineUpdate::GdprExport(
                    GdprExportUpdate::UserPresenceUnavailable,
                ));
                return Some(ActionResult::NavigateTo(self.engine.current_screen()));
            }
            _ => return None,
        };
        if confirmed {
            self.confirm_pin_failures = 0;
            let _ = self
                .engine
                .apply_update(EngineUpdate::GdprExport(GdprExportUpdate::Verified));
            if matches!(self.engine.engine_output(), Some(EngineOutput::Gdpr(_))) {
                return Some(self.complete_privacy());
            }
            return Some(ActionResult::NavigateTo(self.engine.current_screen()));
        }
        if let Some(locked) = self.record_confirm_pin_miss() {
            return Some(locked);
        }
        let _ = self
            .engine
            .apply_update(EngineUpdate::GdprExport(GdprExportUpdate::PinRejected));
        Some(ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// The shell's answer to the check that guards a first app password
    /// (#766). An unavailable check still sets it: a lost sealed secret
    /// leaves the app password as the only way back to exporting (#739).
    fn handle_first_password_presence(&mut self, event: &Event) -> Option<ActionResult> {
        let confirmed = match event {
            Event::UserPresenceSecretCreated { secret } => self
                .vauchi
                .enroll_user_presence_secret(secret.as_bytes())
                .unwrap_or(false),
            Event::UserPresenceSecretRevealed { secret } => self
                .vauchi
                .confirm_user_presence_secret(secret.as_bytes())
                .unwrap_or(false),
            Event::HardwareUnavailable { transport } if transport == USER_PRESENCE_TRANSPORT => {
                true
            }
            Event::UserPresenceCheckCancelled => {
                self.pending_first_password = None;
                return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
            }
            _ => return None,
        };
        let password = self.pending_first_password.take()?;
        if !confirmed {
            if let Some(locked) = self.record_confirm_pin_miss() {
                return Some(locked);
            }
            return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
        }
        if let Err(e) = self.vauchi.setup_app_password(&password) {
            return Some(ActionResult::ShowAlert {
                title: self.t("error.title"),
                message: format!("Could not set password: {e}"),
            });
        }
        self.confirm_pin_failures = 0;
        Some(self.finish_password_change(true))
    }

    /// Counts a wrong PIN at any re-confirmation step and locks the app
    /// after as many misses as the lock screen allows, so someone holding
    /// the unlocked phone cannot keep guessing (#469).
    pub(super) fn record_confirm_pin_miss(&mut self) -> Option<ActionResult> {
        self.confirm_pin_failures += 1;
        if self.confirm_pin_failures < DEFAULT_LOCK_MAX_ATTEMPTS {
            return None;
        }
        self.confirm_pin_failures = 0;
        self.handle_app_backgrounded().map(ActionResult::NavigateTo)
    }

    /// Runs one confirmed readable export: the JSON, every contact as
    /// vCard 4.0, or the heirloom document.
    pub(super) fn export_as(&mut self, choice: GdprChoice) -> ActionResult {
        match choice {
            GdprChoice::Export => match self.vauchi.export_personal_data() {
                Ok(export) => match serde_json::to_string_pretty(&export) {
                    Ok(json) => ActionResult::GdprExportComplete { json },
                    Err(_) => self.export_failed("privacy.export_failed_serialize"),
                },
                Err(_) => self.export_failed("privacy.export_failed_read"),
            },
            GdprChoice::ExportVcard => match self.vauchi.list_contacts() {
                Ok(contacts) => ActionResult::VcardExportComplete {
                    vcf: contacts
                        .iter()
                        .map(|contact| {
                            vauchi_core::contact_card::vcard::export_contact_vcard(contact) + "\r\n"
                        })
                        .collect(),
                },
                Err(_) => self.export_failed("privacy.export_failed_read"),
            },
            GdprChoice::Heirloom => match self.vauchi.heirloom_book() {
                Ok(book) => {
                    let text = vauchi_core::api::HeirloomText {
                        document_title: self.t("heirloom.document_title"),
                        exchanged_on: self.t("heirloom.exchanged_on"),
                        met_at: self.t("heirloom.met_at"),
                    };
                    ActionResult::HeirloomExportComplete {
                        html: vauchi_core::api::render_heirloom_html(&book, &text),
                    }
                }
                Err(_) => self.export_failed("privacy.heirloom.failed"),
            },
            _ => self.export_failed("privacy.export_failed_read"),
        }
    }

    /// The full data export sealed under the user's export password.
    pub(super) fn export_encrypted(&mut self, mut password: String) -> ActionResult {
        let sealed = self.vauchi.export_personal_data_encrypted(&password);
        password.zeroize();
        match sealed {
            Ok(data) => ActionResult::EncryptedExportComplete { data },
            Err(_) => self.export_failed("privacy.export_failed_read"),
        }
    }

    /// Ends the one export the confirmation covered, so its passwords and
    /// choice do not outlive it.
    pub(super) fn finish_export(&mut self, result: ActionResult) -> ActionResult {
        let _ = self
            .engine
            .apply_update(EngineUpdate::GdprExport(GdprExportUpdate::Finished));
        result
    }

    fn export_failed(&self, key: &str) -> ActionResult {
        ActionResult::ShowToast {
            message: self.t(key),
            undo_action_id: None,
            undo_label: None,
        }
    }
}
