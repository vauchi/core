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
use zeroize::Zeroize;

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
