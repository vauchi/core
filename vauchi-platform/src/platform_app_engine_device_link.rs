// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pair 5 device-link session wiring on `PlatformAppEngine`.
//! Extracted from `platform_app_engine.rs` to keep that file under
//! its size baseline. Pair 5 of
//! `_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens/`.
//!
//! Two halves:
//!
//! - Lifecycle + dispatch on `PlatformAppEngine` — the
//!   `ensure_device_link_session` / `cancel_device_link_session`
//!   pair that `after_screen_transition` calls when navigation
//!   enters or leaves `AppScreen::DeviceLinking`, plus
//!   `dispatch_device_link_side_effects` which translates the
//!   engine's typed `ActionResult` variants into
//!   `MobileDeviceLinkSession` calls.
//! - The `DeviceLinkEngineBridge` listener — implements
//!   `DeviceLinkSessionListener` (the UniFFI surface trait) and
//!   forwards each cycle-thread callback into the active
//!   `AppEngine`'s receiver-side bridge methods.

use std::sync::{Arc, Mutex};

use vauchi_app::ui::{ActionResult, AppEngine};

use crate::MobileDeviceLinkSession;
use crate::error::MobileError;
use crate::mobile_device_link_session::DeviceLinkSessionListener;
use crate::platform_app_engine::{DirectListenerSlot, PlatformAppEngine};

impl PlatformAppEngine {
    /// Lazily create + start the `MobileDeviceLinkSession` and wire
    /// the `DeviceLinkEngineBridge` listener so cycle-thread
    /// callbacks reach the active engine. Idempotent: a no-op when
    /// a session is already running.
    pub(crate) fn ensure_device_link_session(&self) -> Result<(), MobileError> {
        let mut slot = self
            .device_link_session()
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
        if slot.is_some() {
            return Ok(());
        }
        // Drive the engine into QrPending up-front so the screen
        // renders a generating-link spinner until `on_qr_ready`
        // lands. Errors (off-screen) are non-fatal.
        let _ = {
            let mut engine = self.engine().lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            engine.device_link_qr_pending()
        };
        let session = self.create_device_link_session_initiator()?;
        let bridge = DeviceLinkEngineBridge {
            engine: Arc::clone(self.engine()),
            direct_listener: Arc::clone(self.direct_listener()),
        };
        session.set_listener(Box::new(bridge));
        session.start();
        *slot = Some(session);
        Ok(())
    }

    /// Cancel + drop the active `MobileDeviceLinkSession`.
    /// Cancellation is idempotent.
    pub(crate) fn cancel_device_link_session(&self) {
        let session_to_cancel = self
            .device_link_session()
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(session) = session_to_cancel {
            session.cancel();
        }
    }

    /// Translate the engine's typed device-link `ActionResult`
    /// variants into calls on the active `MobileDeviceLinkSession`.
    /// The cycle thread will then push the resulting state changes
    /// back through the `DeviceLinkEngineBridge` listener.
    pub(crate) fn dispatch_device_link_side_effects(
        &self,
        result: &ActionResult,
    ) -> Result<(), MobileError> {
        match result {
            ActionResult::DeviceLinkConfirmManual { code } => {
                if let Some(session) = self.device_link_session_clone()? {
                    let _ = session.confirm_manual(code.clone(), now_unix_secs());
                }
            }
            ActionResult::DeviceLinkDeny => {
                if let Some(session) = self.device_link_session_clone()? {
                    session.deny();
                }
            }
            ActionResult::DeviceLinkRetry => {
                // Engine already moved to QrPending. Cancel the
                // stale session (idempotent) and create a fresh
                // one — the new cycle thread fires `on_qr_ready`
                // to advance the engine.
                self.cancel_device_link_session();
                self.ensure_device_link_session()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn device_link_session_clone(
        &self,
    ) -> Result<Option<Arc<MobileDeviceLinkSession>>, MobileError> {
        Ok(self
            .device_link_session()
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?
            .clone())
    }

    // ── Test-only helpers ──────────────────────────────────────────
    //
    // These bridge entry points exist so integration tests can drive
    // the device-link state without spinning up a real cycle thread.
    // They are not part of the UniFFI surface — Swift / Kotlin
    // frontends never see them, and the production bridge
    // (`DeviceLinkEngineBridge`) goes straight to
    // `AppEngine::device_link_*`. The `_for_test` suffix mirrors
    // the multi-stage convention.

    /// Test-only: simulate `on_qr_ready` from the cycle thread.
    #[doc(hidden)]
    pub fn apply_device_link_qr_ready_for_test(
        &self,
        qr_data: String,
        expires_at: u64,
    ) -> Result<(), MobileError> {
        let mut engine = self.engine().lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let _ = engine.device_link_qr_ready(qr_data, expires_at);
        Ok(())
    }

    /// Test-only: simulate `on_confirmation_required`.
    #[doc(hidden)]
    pub fn apply_device_link_request_received_for_test(
        &self,
        device_name: String,
        confirmation_code: String,
        challenge_hex: String,
    ) -> Result<(), MobileError> {
        let mut engine = self.engine().lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let _ = engine.device_link_request_received(device_name, confirmation_code, challenge_hex);
        Ok(())
    }

    /// Test-only: simulate `on_failed("qr_expired")`.
    #[doc(hidden)]
    pub fn apply_device_link_qr_expired_for_test(&self) -> Result<(), MobileError> {
        let mut engine = self.engine().lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let _ = engine.device_link_qr_expired();
        Ok(())
    }

    /// Test-only: simulate `on_failed` (generic message).
    #[doc(hidden)]
    pub fn apply_device_link_failed_for_test(&self, reason: String) -> Result<(), MobileError> {
        let mut engine = self.engine().lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let _ = engine.device_link_failed(reason);
        Ok(())
    }

    /// Test-only: simulate `on_completed` from the cycle thread.
    #[doc(hidden)]
    pub fn apply_device_link_completed_for_test(&self) -> Result<(), MobileError> {
        let mut engine = self.engine().lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let _ = engine.device_link_completed();
        Ok(())
    }

    /// Test-only: returns `true` when a `MobileDeviceLinkSession` is
    /// currently held. Used to assert lifecycle correctness around
    /// navigation in/out of the device-linking screen.
    #[doc(hidden)]
    pub fn device_link_session_is_active_for_test(&self) -> bool {
        self.device_link_session()
            .lock()
            .map(|slot| slot.is_some())
            .unwrap_or(false)
    }

    /// Test-only: cancel the active session without leaving the
    /// device-linking screen. Bridge-forwarding tests use this to
    /// stop the live cycle thread from racing the test driver
    /// before asserting state pushed via the
    /// `apply_device_link_*_for_test` helpers.
    #[doc(hidden)]
    pub fn cancel_device_link_session_for_test(&self) {
        self.cancel_device_link_session();
    }
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Bridge listener that forwards `MobileDeviceLinkSession`
/// cycle-thread callbacks into the active `AppEngine`'s
/// receiver-side bridge methods. Pair 5 sibling of
/// `MultiStageEngineBridge`.
///
/// Notification: every state-mutating callback ends with
/// `on_screens_invalidated(["device_linking"])` so the frontend
/// re-fetches `current_screen_json`.
struct DeviceLinkEngineBridge {
    engine: Arc<Mutex<AppEngine>>,
    direct_listener: DirectListenerSlot,
}

impl DeviceLinkEngineBridge {
    fn notify(&self) {
        let listener = self
            .direct_listener
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        if let Some(listener) = listener {
            listener.on_screens_invalidated(vec!["device_linking".into()]);
        }
    }
}

impl DeviceLinkSessionListener for DeviceLinkEngineBridge {
    fn on_qr_ready(&self, qr_data: String, expires_at_unix: u64) {
        let applied = match self.engine.lock() {
            Ok(mut e) => e.device_link_qr_ready(qr_data, expires_at_unix).is_some(),
            Err(_) => false,
        };
        if applied {
            self.notify();
        }
    }

    fn on_confirmation_required(
        &self,
        device_name: String,
        confirmation_code: String,
        identity_fingerprint: String,
        proximity_challenge: Vec<u8>,
    ) {
        // Concatenate fingerprint + challenge for the engine's hex
        // payload — the receiver-side ConfirmingDevice screen
        // renders both via the engine's `challenge_hex` field; the
        // iOS `ProximityVerificationView` would use both halves to
        // validate ultrasonic responses (deferred). For the
        // manual-only path we only need to round-trip the bytes;
        // the engine treats it as opaque hex.
        let challenge_hex = format!(
            "{}:{}",
            identity_fingerprint,
            hex::encode(&proximity_challenge)
        );
        let applied = match self.engine.lock() {
            Ok(mut e) => e
                .device_link_request_received(device_name, confirmation_code, challenge_hex)
                .is_some(),
            Err(_) => false,
        };
        if applied {
            self.notify();
        }
    }

    fn on_request_sent(&self, _confirmation_code: String) {
        // Responder-side callback — Phase 1 cycle thread does not
        // fire it. Recorded for completeness; no engine state
        // change.
    }

    fn on_completed(&self, _device_name: String, _device_index: u32) {
        let applied = match self.engine.lock() {
            Ok(mut e) => e.device_link_completed().is_some(),
            Err(_) => false,
        };
        if applied {
            self.notify();
        }
    }

    fn on_failed(&self, reason: String) {
        // Stable identifiers (`"qr_expired"`, `"user_denied"`,
        // `"user_confirm_timeout"`, `"cancelled"`) route to specific
        // engine states; everything else falls through to the
        // generic failed screen with the reason as the message.
        let applied = match self.engine.lock() {
            Ok(mut e) => match reason.as_str() {
                "qr_expired" => e.device_link_qr_expired().is_some(),
                _ => e.device_link_failed(reason).is_some(),
            },
            Err(_) => false,
        };
        if applied {
            self.notify();
        }
    }

    fn on_session_ended(&self) {
        // Terminal-only invalidation; the prior `on_completed` /
        // `on_failed` already set the engine state.
        self.notify();
    }
}
