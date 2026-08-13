// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine-owned device-link **initiator** (slice 32l T3.1b).
//!
//! The poll-driven [`DeviceLinkInitiatorMachine`] lives on `AppEngine`
//! so every frontend — mobile (`PlatformAppEngine` delegates here) and
//! desktop/C-ABI (`vauchi-cabi` wraps `AppEngine` directly) — shares one
//! source of truth. There is no cycle thread and no listener bridge.
//!
//! - [`AppEngine::sync_device_link_lifecycle`] ensures / cancels the
//!   machine on entry / exit of [`AppScreen::DeviceLinking`] (called from
//!   `navigate_to_internal`, mirroring the link-responder lifecycle).
//! - [`AppEngine::advance_device_link_session`] runs one non-blocking
//!   relay step; called from `poll_notifications`.
//! - [`AppEngine::dispatch_device_link_side_effects`] feeds the engine's
//!   typed `ActionResult::DeviceLink*` into the machine.
//! - [`AppEngine::apply_initiator_event`] maps an [`InitiatorEvent`] onto
//!   the `device_link_*` screen handlers (see `app_engine/device_link.rs`).

use super::{AppEngine, AppScreen};
use crate::orchestrator::device_link_machine::DeviceLinkPersistence;
use crate::orchestrator::device_link_machine::{DeviceLinkInitiatorMachine, InitiatorEvent};
use crate::orchestrator::device_link_relay::DeviceLinkBroker;
use crate::ui::ActionResult;
use vauchi_core::network::HttpTransport;

/// QR-expiry / relay-listen budget (ADR-035 device-link window = 300 s).
pub(crate) const QR_WINDOW_SECS: u64 = 300;

/// Holds the initiator machine plus the relay broker it offered on;
/// `advance` polls the same broker each tick. Boxed as a trait object so
/// tests can inject a fake broker (mirrors the responder holder).
pub(crate) struct DeviceLinkInitiatorHolder {
    machine: DeviceLinkInitiatorMachine,
    transport: Box<dyn DeviceLinkBroker + Send>,
}

impl AppEngine {
    /// Build / cancel the initiator machine on entry / exit of the
    /// `DeviceLinking` screen. Mirrors `sync_link_responder_lifecycle`.
    pub(super) fn sync_device_link_lifecycle(&mut self, old: &AppScreen, new: &AppScreen) {
        let was = matches!(old, AppScreen::DeviceLinking);
        let is = matches!(new, AppScreen::DeviceLinking);
        match (was, is) {
            (true, false) => self.cancel_device_link_session(),
            (false, true) => self.ensure_device_link_session(),
            _ => {}
        }
    }

    /// Lazily build the initiator machine and render the
    /// generating-link (QrPending) screen. Idempotent: a no-op when a
    /// machine is already held. Off-screen build errors are non-fatal —
    /// the screen falls back to the `DeviceLinkingEngine` default.
    pub(super) fn ensure_device_link_session(&mut self) {
        if self.device_link_initiator.is_some() {
            return;
        }
        let (initiator, transport, identity_id, persistence) =
            match self.build_device_link_initiator() {
                Ok(parts) => parts,
                Err(e) => {
                    tracing::warn!("device-link: cannot start initiator: {e}");
                    // Surface it rather than falling back to the engine
                    // default: that default is the QrPending spinner, and
                    // with no machine stored every later
                    // `advance_device_link_session()` returns at its `None`
                    // arm, so nothing ever clears it. `failure_detail` maps
                    // an unrecognised reason to honest generic copy, so the
                    // raw build error never reaches the screen
                    // (2026-08-13-device-link-generation-never-completes).
                    let _ = self.device_link_failed(e);
                    return;
                }
            };
        // Render the generating-link spinner until the offer lands.
        let _ = self.device_link_qr_pending();
        // No relay I/O here — the offer posts on the first
        // `advance_device_link_session()`, so navigation never blocks.
        let machine = DeviceLinkInitiatorMachine::new(
            initiator,
            identity_id,
            QR_WINDOW_SECS,
            Some(persistence),
        );
        self.device_link_initiator = Some(DeviceLinkInitiatorHolder {
            machine,
            transport: Box::new(transport),
        });
    }

    /// Cancel + drop the active initiator machine. Idempotent. `pub`
    /// so binding crates (cabi, platform) can cancel without a nav-out.
    pub fn cancel_device_link_session(&mut self) {
        if let Some(mut holder) = self.device_link_initiator.take() {
            let _ = holder.machine.cancel();
        }
    }

    /// One non-blocking relay step. Called from `poll_notifications`.
    /// Returns true if the device-linking screen changed.
    pub(crate) fn advance_device_link_session(&mut self) -> bool {
        let now = self.vauchi.clock().unix_seconds();
        let event = match self.device_link_initiator.as_mut() {
            Some(holder) => holder.machine.advance(holder.transport.as_ref(), now),
            None => return false,
        };
        // Only transitions, never the idle tick: this runs every ~30s on any
        // screen. Without it a relay rejection is invisible — the offer
        // failing with HTTP 502 was diagnosable only by instrumenting this
        // line (2026-08-13-device-link-generation-never-completes).
        if !matches!(event, InitiatorEvent::None) {
            tracing::info!("device-link: advance -> {event:?}");
        }
        self.apply_initiator_event(event)
    }

    /// Translate the engine's typed device-link `ActionResult`s into
    /// machine inputs (confirm / deny / retry).
    pub(super) fn dispatch_device_link_side_effects(&mut self, result: &ActionResult) {
        let now = self.vauchi.clock().unix_seconds();
        let event = match (result, self.device_link_initiator.as_mut()) {
            (ActionResult::DeviceLinkConfirmManual { code }, Some(holder)) => {
                Some(holder.machine.confirm_manual(code.clone(), now))
            }
            (ActionResult::DeviceLinkDeny, Some(holder)) => Some(holder.machine.deny()),
            _ => None,
        };
        if let Some(event) = event {
            self.apply_initiator_event(event);
        }
        if matches!(result, ActionResult::DeviceLinkRetry) {
            // Engine already moved to QrPending. Drop the stale machine
            // and start a fresh one — its offer fires QrReady.
            self.cancel_device_link_session();
            self.ensure_device_link_session();
        }
    }

    /// Map an [`InitiatorEvent`] onto the engine's `device_link_*`
    /// screen handlers. Returns true if a handler applied.
    fn apply_initiator_event(&mut self, event: InitiatorEvent) -> bool {
        match event {
            InitiatorEvent::None => false,
            InitiatorEvent::QrReady {
                qr_data,
                expires_at_unix,
            } => {
                // Prefer the full join invitation URL for the QR payload.
                // The bare qr_data is retained as a fallback only if the
                // machine has not yet produced an invitation.
                let invitation_url = self
                    .device_link_initiator
                    .as_ref()
                    .and_then(|holder| holder.machine.join_invitation())
                    .map(|invitation| invitation.to_url())
                    .unwrap_or(qr_data);
                self.device_link_qr_ready(invitation_url, expires_at_unix)
                    .is_some()
            }
            InitiatorEvent::ConfirmationRequired {
                device_name,
                confirmation_code,
                identity_fingerprint,
                challenge,
            } => {
                // Concatenate fingerprint + challenge into the engine's
                // opaque hex payload (matches the prior bridge encoding).
                let challenge_hex = format!("{identity_fingerprint}:{}", hex::encode(&challenge));
                self.device_link_request_received(device_name, confirmation_code, challenge_hex)
                    .is_some()
            }
            InitiatorEvent::Completed { .. } => self.device_link_completed().is_some(),
            InitiatorEvent::Failed { reason } => match reason.as_str() {
                "qr_expired" => self.device_link_qr_expired().is_some(),
                _ => self.device_link_failed(reason).is_some(),
            },
        }
    }

    /// Build the parts the initiator machine needs from the engine's
    /// `Vauchi`: initiator state, relay transport, identity id, and the
    /// persistence handle (storage path + key from the config).
    fn build_device_link_initiator(
        &self,
    ) -> Result<
        (
            vauchi_core::exchange::DeviceLinkInitiator,
            HttpTransport,
            String,
            DeviceLinkPersistence,
        ),
        String,
    > {
        let identity = self
            .vauchi
            .identity()
            .ok_or_else(|| "identity not initialized".to_string())?;
        let registry = self
            .vauchi
            .storage()
            .device()
            .load_device_registry()
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| identity.initial_device_registry());
        let initiator =
            identity.create_device_link_initiator(registry, self.vauchi.clock().unix_seconds());
        let identity_id = hex::encode(identity.signing_public_key());

        let connect_timeout_ms = self.vauchi.config().relay.connect_timeout_ms;
        let transport = self.vauchi.build_relay_transport(
            &self.vauchi.config().relay.server_url,
            connect_timeout_ms.max(10_000),
        );

        let storage_key = self
            .vauchi
            .config()
            .storage_key
            .clone()
            .ok_or_else(|| "no storage key configured".to_string())?;
        let persistence = DeviceLinkPersistence {
            storage_path: self.vauchi.config().storage_path.clone(),
            storage_key,
        };
        Ok((initiator, transport, identity_id, persistence))
    }

    /// Whether an initiator machine is currently held.
    pub fn device_link_initiator_active(&self) -> bool {
        self.device_link_initiator.is_some()
    }
}

// INLINE_TEST_REQUIRED: injects a fake broker into the pub(crate)
// DeviceLinkInitiatorHolder and reads the private device_link_initiator
// field — the injection seam is not reachable from tests/.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::component::Component;
    use vauchi_core::Vauchi;
    use vauchi_core::network::NetworkError;

    /// Broker whose offer always succeeds with a fixed rendezvous code, so the
    /// initiator reaches `AwaitingClaim` (where `join_invitation()` is `Some`).
    struct FakeBroker {
        code: String,
    }
    impl DeviceLinkBroker for FakeBroker {
        fn exchange_offer(&self, _p: &str, _e: Option<u64>) -> Result<String, NetworkError> {
            Ok(self.code.clone())
        }
        fn exchange_claim(&self, _c: &str, _r: &str) -> Result<String, NetworkError> {
            Ok(String::new())
        }
        fn exchange_complete(&self, _c: &str) -> Result<Option<String>, NetworkError> {
            Ok(None)
        }
    }

    // @internal
    // Regression for the device-link-join QR: after a *successful* offer the
    // rendered QR must carry the full `vauchi://device-link?qr=…&code=…` deep
    // link (which iOS/Android can route), NOT the codeless bare `qr_data`
    // fallback. See backlog 2026-07-27-device-link-exchange-rendezvous-hang.
    #[test]
    fn qr_ready_renders_full_vauchi_deep_link_not_bare_qr() {
        let mut app = AppEngine::new(Vauchi::in_memory().expect("in-memory vauchi"));
        app.vauchi.create_identity("Alice").expect("identity");

        // Enter DeviceLinking so the active engine is the DeviceLinkingEngine
        // (its `apply_update` receives the QrReady render).
        app.navigate_to(AppScreen::DeviceLinking);

        // Build the initiator holder manually with a fake broker. (The real
        // `ensure_device_link_session` needs a configured `storage_key` for
        // persistence, which the in-memory test Vauchi lacks — orthogonal to
        // the QR payload under test, so we pass `persistence: None`.)
        let (initiator, identity_id) = {
            let identity = app.vauchi.identity().expect("identity");
            let registry = identity.initial_device_registry();
            let now = app.vauchi.clock().unix_seconds();
            let initiator = identity.create_device_link_initiator(registry, now);
            (initiator, hex::encode(identity.signing_public_key()))
        };
        let machine = DeviceLinkInitiatorMachine::new(initiator, identity_id, QR_WINDOW_SECS, None);
        app.device_link_initiator = Some(DeviceLinkInitiatorHolder {
            machine,
            // Fake broker whose offer succeeds → machine reaches AwaitingClaim.
            transport: Box::new(FakeBroker {
                code: "BROKER42".to_string(),
            }),
        });

        // First advance posts the offer → QrReady → renders the waiting screen.
        assert!(
            app.advance_device_link_session(),
            "advance renders the QR-ready screen"
        );

        let screen = app.engine.current_screen();
        let qr_data = screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::QrCode { data, .. } => Some(data.clone()),
                _ => None,
            })
            .expect("a QrCode component on the waiting-for-request screen");

        assert!(
            qr_data.starts_with("vauchi://device-link"),
            "QR must carry the full vauchi:// deep link (iOS/Android routable), \
             not the codeless bare qr_data fallback; got prefix: {}",
            &qr_data[..qr_data.len().min(30)]
        );
    }

    // Regression pinned on the physical rig (2026-07-28): with the relay
    // OHTTP data-plane returning 502, the DeviceLinking screen presented a
    // *bare, relay-less* QR (`DeviceLinkQR::to_data_string`) that looks
    // scannable but has no `vauchi://` scheme and no rendezvous `code` — so
    // no peer can ever join it. The screen must instead render the honest
    // "generating link" spinner until the relay produces a real invitation
    // (`QrReady` → WaitingForRequest); a failed offer surfaces link_failed.
    // No QR may appear before a successful relay offer.
    // @internal
    #[test]
    fn device_linking_shows_generating_spinner_not_a_bare_qr_before_relay_offer() {
        let mut app = AppEngine::new(Vauchi::in_memory().expect("in-memory vauchi"));
        app.vauchi.create_identity("Alice").expect("identity");

        // Enter DeviceLinking. No relay offer has landed yet (and the
        // in-memory Vauchi cannot even post one), so nothing scannable exists.
        app.navigate_to(AppScreen::DeviceLinking);

        let screen = app.engine.current_screen();
        let has_qr = screen
            .components
            .iter()
            .any(|c| matches!(c, Component::QrCode { .. }));
        assert!(
            !has_qr,
            "DeviceLinking must show the generating-link spinner (no QR) until the \
             relay produces a real vauchi:// invitation — never a bare, non-routable \
             QR; got screen_id={}",
            screen.screen_id
        );
    }
}
