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
use crate::ui::ActionResult;
use vauchi_core::network::HttpTransport;

/// QR-expiry / relay-listen budget (ADR-035 device-link window = 300 s).
pub(crate) const RELAY_TIMEOUT_SECS: u64 = 300;

/// Holds the initiator machine plus the relay transport it offered on;
/// `advance` polls the same transport each tick.
pub(crate) struct DeviceLinkInitiatorHolder {
    machine: DeviceLinkInitiatorMachine,
    transport: HttpTransport,
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
                    log::warn!("device-link: cannot start initiator: {e}");
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
            RELAY_TIMEOUT_SECS,
            Some(persistence),
        );
        self.device_link_initiator = Some(DeviceLinkInitiatorHolder { machine, transport });
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
            Some(holder) => holder.machine.advance(&holder.transport, now),
            None => return false,
        };
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

        let relay_url = self.vauchi.config().relay.server_url.clone();
        let connect_timeout_ms = self.vauchi.config().relay.connect_timeout_ms;
        let transport = self
            .vauchi
            .build_relay_transport(relay_url, connect_timeout_ms.max(10_000));

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
