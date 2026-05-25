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

use super::{AppEngine, AppScreen};
use crate::ui::ScreenModel;
use crate::ui::WorkflowEngine;
use crate::ui::device_linking::DeviceLinkingEngine;

impl AppEngine {
    /// Cycle-thread bridge: signal that a fresh device-link session
    /// has been spawned and is preparing the QR.
    pub fn device_link_qr_pending(&mut self) -> Option<ScreenModel> {
        let dl = self.device_linking_engine_mut()?;
        dl.transition_to_qr_pending();
        Some(dl.current_screen())
    }

    /// Cycle-thread bridge for `DeviceLinkSessionListener::on_qr_ready`
    /// — the QR is ready and the session is now waiting for a peer
    /// scan. `expires_at` is unix-seconds (ADR-035 5-minute window).
    pub fn device_link_qr_ready(
        &mut self,
        qr_data: String,
        expires_at: u64,
    ) -> Option<ScreenModel> {
        let dl = self.device_linking_engine_mut()?;
        dl.transition_to_waiting_for_request(qr_data, expires_at);
        Some(dl.current_screen())
    }

    /// Cycle-thread bridge for the `"qr_expired"` failure branch of
    /// `DeviceLinkSessionListener::on_failed` — the QR window
    /// elapsed before any peer connected.
    pub fn device_link_qr_expired(&mut self) -> Option<ScreenModel> {
        let dl = self.device_linking_engine_mut()?;
        dl.transition_to_qr_expired();
        Some(dl.current_screen())
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
        let dl = self.device_linking_engine_mut()?;
        dl.transition_to_confirming_device(device_name, confirmation_code, challenge_hex);
        Some(dl.current_screen())
    }

    /// Cycle-thread bridge for
    /// `DeviceLinkSessionListener::on_completed` — terminal success.
    /// Distinct from `device_link_sync_complete` in that it can fire
    /// from any non-terminal step (the cycle thread does not always
    /// pass through the legacy `Syncing` state).
    pub fn device_link_completed(&mut self) -> Option<ScreenModel> {
        let dl = self.device_linking_engine_mut()?;
        dl.transition_to_link_success();
        Some(dl.current_screen())
    }

    /// Cycle-thread bridge for
    /// `DeviceLinkSessionListener::on_failed` — terminal failure.
    /// `reason` is the listener's stable identifier
    /// (`"user_denied"`, `"user_confirm_timeout"`, `"cancelled"`,
    /// relay/decode errors).
    pub fn device_link_failed(&mut self, reason: String) -> Option<ScreenModel> {
        let dl = self.device_linking_engine_mut()?;
        dl.transition_to_link_failed(reason);
        Some(dl.current_screen())
    }

    fn device_linking_engine_mut(&mut self) -> Option<&mut DeviceLinkingEngine> {
        if self.screen != AppScreen::DeviceLinking {
            return None;
        }
        self.engine
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<DeviceLinkingEngine>())
    }
}
