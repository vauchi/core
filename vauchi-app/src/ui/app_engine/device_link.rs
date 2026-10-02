// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-link bridge methods on `AppEngine`. Extracted from
//! `app_engine/mod.rs` to keep that file under its size baseline
//! after the Pair 5 receiver-side state additions.
//!
//! Each bridge mirrors a `DeviceLinkSessionListener` event from
//! `vauchi_app::orchestrator::device_link_session` and pushes the
//! state into the active `DeviceLinkingEngine`. They return `None`
//! when the engine is not on `AppScreen::DeviceLinking` so the
//! caller can ignore stale callbacks after navigation.
//!
//! Pair 5 of
//! `_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens/`.

use super::AppEngine;
use crate::ui::ScreenModel;
use crate::ui::device_link_pin::CONFIRM_PIN_ACTION_ID;
use crate::ui::lock_screen::DEFAULT_LOCK_MAX_ATTEMPTS;
use crate::ui::{
    ActionResult, AppScreen, DeviceLinkUpdate, EngineOutput, EngineUpdate, UserAction,
};
use zeroize::Zeroize;

impl AppEngine {
    /// Cycle-thread bridge: signal that a fresh device-link session
    /// has been spawned and is preparing the QR.
    pub fn device_link_qr_pending(&mut self) -> Option<ScreenModel> {
        self.engine
            .apply_update(EngineUpdate::DeviceLink(DeviceLinkUpdate::QrPending))
            .then(|| self.engine.current_screen())
    }

    /// Cycle-thread bridge for `DeviceLinkSessionListener::on_qr_ready`
    /// — the QR is ready and the session is now waiting for a peer
    /// scan. `expires_at` is unix-seconds (ADR-035 5-minute window).
    /// `invitation_url` is the full `DeviceLinkJoinInvitation` URL to
    /// encode in the QR code.
    pub fn device_link_qr_ready(
        &mut self,
        invitation_url: String,
        expires_at: u64,
    ) -> Option<ScreenModel> {
        self.engine
            .apply_update(EngineUpdate::DeviceLink(DeviceLinkUpdate::QrReady {
                invitation_url,
                expires_at,
            }))
            .then(|| self.engine.current_screen())
    }

    /// Cycle-thread bridge for the `"qr_expired"` failure branch of
    /// `DeviceLinkSessionListener::on_failed` — the QR window
    /// elapsed before any peer connected.
    pub fn device_link_qr_expired(&mut self) -> Option<ScreenModel> {
        self.engine
            .apply_update(EngineUpdate::DeviceLink(DeviceLinkUpdate::QrExpired))
            .then(|| self.engine.current_screen())
    }

    /// Cycle-thread bridge for
    /// `DeviceLinkSessionListener::on_confirmation_required` —
    /// surface the peer's name + confirmation code + proximity
    /// challenge so the user can approve manually.
    pub fn device_link_request_received(
        &mut self,
        device_name: String,
        confirmation_code: String,
        challenge_hex: String,
    ) -> Option<ScreenModel> {
        self.engine
            .apply_update(EngineUpdate::DeviceLink(
                DeviceLinkUpdate::RequestReceived {
                    device_name,
                    confirmation_code,
                    challenge_hex,
                },
            ))
            .then(|| self.engine.current_screen())
    }

    /// Cycle-thread bridge for
    /// `DeviceLinkSessionListener::on_completed` — terminal success.
    /// Distinct from `device_link_sync_complete` in that it can fire
    /// from any non-terminal step (the cycle thread does not always
    /// pass through the legacy `Syncing` state).
    pub fn device_link_completed(&mut self) -> Option<ScreenModel> {
        self.engine
            .apply_update(EngineUpdate::DeviceLink(DeviceLinkUpdate::Completed))
            .then(|| self.engine.current_screen())
    }

    /// Cycle-thread bridge for
    /// `DeviceLinkSessionListener::on_failed` — terminal failure.
    /// `reason` is the listener's stable identifier
    /// (`"user_denied"`, `"user_confirm_timeout"`, `"cancelled"`,
    /// relay/decode errors).
    pub fn device_link_failed(&mut self, reason: String) -> Option<ScreenModel> {
        self.engine
            .apply_update(EngineUpdate::DeviceLink(DeviceLinkUpdate::Failed(reason)))
            .then(|| self.engine.current_screen())
    }

    /// The PIN step that opens device linking (#469): check the PIN against
    /// the one that opened this session, start the link on a match, and
    /// lock the app after as many misses as the lock screen allows, so
    /// someone holding the unlocked phone cannot keep guessing here.
    pub(super) fn intercept_device_link_pin(
        &mut self,
        action: &UserAction,
    ) -> Option<ActionResult> {
        let UserAction::ActionPressed { action_id } = action else {
            return None;
        };
        if action_id != CONFIRM_PIN_ACTION_ID || !matches!(self.screen, AppScreen::DeviceLinking) {
            return None;
        }
        // An empty entry stays with the engine, which keeps the step.
        let Some(EngineOutput::DeviceLinkPin { mut pin }) = self.engine.engine_output() else {
            return None;
        };
        let confirmed = self.vauchi.confirm_session_password(&pin).unwrap_or(false);
        pin.zeroize();
        if confirmed {
            self.device_link_pin_failures = 0;
            let _ = self.device_link_qr_pending();
            #[cfg(all(feature = "network-http", feature = "storage"))]
            self.ensure_device_link_session();
            return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
        }
        self.device_link_pin_failures += 1;
        if self.device_link_pin_failures >= DEFAULT_LOCK_MAX_ATTEMPTS {
            self.device_link_pin_failures = 0;
            if let Some(locked) = self.handle_app_backgrounded() {
                return Some(ActionResult::NavigateTo(locked));
            }
        }
        let _ = self
            .engine
            .apply_update(EngineUpdate::DeviceLink(DeviceLinkUpdate::PinRejected));
        Some(ActionResult::UpdateScreen(self.engine.current_screen()))
    }
}
