// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-link **initiator** wiring on `PlatformAppEngine` (slice 32l
//! Phase 1). The initiator runs as a synchronous
//! [`DeviceLinkInitiatorMachine`] advanced from `poll_notifications` —
//! one non-blocking relay step per tick through the `DeviceLinkBroker`
//! seam. No spawned cycle thread, no `DeviceLinkSessionListener`
//! callback bridge (both retired here; design
//! `_private/docs/designs/2026-05-24-slice-32l-phase-1-device-link-state-machine-design.md`).
//!
//! - `ensure_device_link_session` / `cancel_device_link_session` are
//!   driven by `after_screen_transition` on entry/exit of
//!   `AppScreen::DeviceLinking`.
//! - `advance_device_link_session` is called from `poll_notifications`.
//! - `dispatch_device_link_side_effects` routes the engine's typed
//!   `ActionResult::DeviceLink*` into the machine.
//! - `apply_initiator_event` maps an [`InitiatorEvent`] onto the
//!   engine's `device_link_*` screen handlers (the same mapping the old
//!   `DeviceLinkEngineBridge` performed, now synchronous).

use vauchi_app::orchestrator::device_link_machine::{DeviceLinkInitiatorMachine, InitiatorEvent};
use vauchi_app::ui::ActionResult;
use vauchi_core::network::HttpTransport;

use crate::error::MobileError;
use crate::platform_app_engine::PlatformAppEngine;

/// QR-expiry / relay-listen budget (ADR-035 device-link window = 300 s).
pub(crate) const RELAY_TIMEOUT_SECS: u64 = 300;

/// Holds the initiator machine plus the relay transport it offered on;
/// `advance` polls the same transport each tick.
pub(crate) struct DeviceLinkInitiatorHolder {
    machine: DeviceLinkInitiatorMachine,
    transport: HttpTransport,
}

fn lock_err<E: std::fmt::Display>(e: E) -> MobileError {
    MobileError::Other {
        detail: format!("Lock failed: {e}"),
    }
}

fn now_unix_secs() -> u64 {
    vauchi_core::clock::SystemClock::shared().unix_seconds()
}

impl PlatformAppEngine {
    /// Lazily build + start the initiator machine and apply its first
    /// event (QrReady). Idempotent: a no-op when a machine is running.
    pub(crate) fn ensure_device_link_session(&self) -> Result<(), MobileError> {
        if self
            .device_link_session()
            .lock()
            .map_err(lock_err)?
            .is_some()
        {
            return Ok(());
        }
        // Drive the engine into QrPending so the screen renders a
        // generating-link spinner until the offer lands. Off-screen
        // errors are non-fatal.
        {
            let mut engine = self.engine().lock().map_err(lock_err)?;
            let _ = engine.device_link_qr_pending();
        }

        let (initiator, transport, identity_id, persistence) =
            self.build_device_link_initiator()?;
        let (machine, event) = DeviceLinkInitiatorMachine::start(
            initiator,
            &transport,
            &identity_id,
            now_unix_secs(),
            RELAY_TIMEOUT_SECS,
            Some(persistence),
        );
        self.apply_initiator_event(event);

        *self.device_link_session().lock().map_err(lock_err)? =
            Some(DeviceLinkInitiatorHolder { machine, transport });
        Ok(())
    }

    /// Cancel + drop the active initiator machine. Idempotent.
    pub(crate) fn cancel_device_link_session(&self) {
        let holder = self
            .device_link_session()
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(mut holder) = holder {
            let _ = holder.machine.cancel();
        }
    }

    /// One non-blocking relay step. Called from `poll_notifications`
    /// (never while the engine lock is held). Returns true if the
    /// device-linking screen changed.
    pub(crate) fn advance_device_link_session(&self) -> bool {
        let now = now_unix_secs();
        let event = {
            let mut slot = match self.device_link_session().lock() {
                Ok(slot) => slot,
                Err(_) => return false,
            };
            match slot.as_mut() {
                Some(holder) => holder.machine.advance(&holder.transport, now),
                None => return false,
            }
        };
        self.apply_initiator_event(event)
    }

    /// Translate the engine's typed device-link `ActionResult`s into
    /// machine inputs. Called after the engine lock is released.
    pub(crate) fn dispatch_device_link_side_effects(
        &self,
        result: &ActionResult,
    ) -> Result<(), MobileError> {
        let event = {
            let mut slot = self.device_link_session().lock().map_err(lock_err)?;
            match (result, slot.as_mut()) {
                (ActionResult::DeviceLinkConfirmManual { code }, Some(holder)) => {
                    Some(holder.machine.confirm_manual(code.clone(), now_unix_secs()))
                }
                (ActionResult::DeviceLinkDeny, Some(holder)) => Some(holder.machine.deny()),
                _ => None,
            }
        };
        if let Some(event) = event {
            self.apply_initiator_event(event);
        }
        if matches!(result, ActionResult::DeviceLinkRetry) {
            // Engine already moved to QrPending. Drop the stale machine
            // and start a fresh one — its offer fires QrReady.
            self.cancel_device_link_session();
            self.ensure_device_link_session()?;
        }
        Ok(())
    }

    /// Map an [`InitiatorEvent`] onto the engine's receiver-side
    /// `device_link_*` handlers (same mapping the old bridge did), then
    /// push a `device_linking` screen-invalidation so the frontend
    /// re-fetches. Returns true if a handler applied.
    fn apply_initiator_event(&self, event: InitiatorEvent) -> bool {
        let applied = {
            let mut engine = match self.engine().lock() {
                Ok(engine) => engine,
                Err(_) => return false,
            };
            match event {
                InitiatorEvent::None => false,
                InitiatorEvent::QrReady {
                    qr_data,
                    expires_at_unix,
                } => engine
                    .device_link_qr_ready(qr_data, expires_at_unix)
                    .is_some(),
                InitiatorEvent::ConfirmationRequired {
                    device_name,
                    confirmation_code,
                    identity_fingerprint,
                    challenge,
                } => {
                    // Concatenate fingerprint + challenge for the
                    // engine's opaque hex payload (matches the prior
                    // bridge encoding).
                    let challenge_hex =
                        format!("{identity_fingerprint}:{}", hex::encode(&challenge));
                    engine
                        .device_link_request_received(device_name, confirmation_code, challenge_hex)
                        .is_some()
                }
                InitiatorEvent::Completed { .. } => engine.device_link_completed().is_some(),
                InitiatorEvent::Failed { reason } => match reason.as_str() {
                    "qr_expired" => engine.device_link_qr_expired().is_some(),
                    _ => engine.device_link_failed(reason).is_some(),
                },
            }
        };
        if applied {
            self.notify_device_linking_invalidated();
        }
        applied
    }

    fn notify_device_linking_invalidated(&self) {
        let listener = self
            .direct_listener()
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        if let Some(listener) = listener {
            listener.on_screens_invalidated(vec!["device_linking".into()]);
        }
    }
}
