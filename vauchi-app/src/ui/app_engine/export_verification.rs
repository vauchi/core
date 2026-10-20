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
use crate::ui::{ActionResult, EngineOutput, EngineUpdate, GdprExportUpdate, UserAction};
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
            return Some(self.complete_privacy());
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
}
