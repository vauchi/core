// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine-owned BLE handshake machine (slice 32m T2.2b).
//!
//! The [`BleHandshakeMachine`] lives on `AppEngine` so every
//! frontend — mobile (`PlatformAppEngine` delegates here) and
//! desktop/C-ABI (`vauchi-cabi` wraps `AppEngine` directly) —
//! shares one source of truth. There is no cycle thread and no
//! delegate-callback trait.
//!
//! Mirrors the slice 32m T1.2b multi-stage integration
//! ([`super::multi_stage_exchange`]) verbatim. The 32l Phase 1a /
//! 1b split applies here too:
//!
//! - **T2.2b (Phase 1a equivalent):** add the AppEngine integration
//!   so cabi/windows get the event-driven path. The Android frontend
//!   still owns its parallel `MobileBleExchangeSession` cycle —
//!   parallel paths until Phase 4.2 rewires `AndroidBleDelegate.kt`.
//! - **T2.2c (Phase 1b equivalent):** PlatformAppEngine routes
//!   `Event::BleMtuNegotiated` (and the rest of the BLE event
//!   subset) through the AppEngine machine so the new path has a
//!   production-shaped consumer.
//! - **T3.1 (Phase 4 equivalent):** delete `mobile_ble.rs` outright
//!   once Android consumer rewires (Phase 4.2).
//!
//! Surface:
//!
//! - [`AppEngine::ensure_ble_handshake_session`] — builds the
//!   machine for a given role + identity. Called by frontends when
//!   they observe `BleConnected` (cabi) or by PlatformAppEngine on
//!   BLE-eligible screen entry.
//! - [`AppEngine::cancel_ble_handshake_session`] — terminates the
//!   active session. Drains a `Command::BleDisconnect` into
//!   `pending_commands`.
//! - [`AppEngine::forward_ble_hardware_event`] — routes a
//!   `vauchi_core::Event` into the machine, drains any emitted
//!   commands into `pending_commands`. Returns the
//!   `BleMachineEvent` so the engine can flip chrome.
//! - [`AppEngine::ble_handshake_session_active`] +
//!   [`AppEngine::ble_machine_phase`] — read-side test seams and
//!   cabi screen-rendering helpers.

use super::AppEngine;
use crate::orchestrator::ble_handshake_machine::{
    BleHandshakeMachine, BleMachineEvent, BleMachinePhase, BleRole,
};
use vauchi_core::Event;
use vauchi_core::crypto::X3DHKeyPair;
use vauchi_core::exchange::BleCardPayload;

/// Wraps the machine on `AppEngine`. Same shape as
/// `multi_stage_exchange::MultiStageHolder`.
pub(crate) struct BleHandshakeHolder {
    machine: BleHandshakeMachine,
}

impl AppEngine {
    /// Lazily build the BLE handshake machine. Idempotent: a no-op
    /// when a machine is already held. Frontends drive this on
    /// `Event::BleConnected` (cabi) or PlatformAppEngine drives it
    /// on BLE-eligible screen entry.
    ///
    /// `role` selects initiator vs responder (the frontend decides
    /// by who-initiated-the-connection convention — same convention
    /// `MobileBleExchangeSession::set_responder` used pre-32m).
    pub fn ensure_ble_handshake_session(
        &mut self,
        role: BleRole,
        identity_key: [u8; 32],
        identity_x3dh: X3DHKeyPair,
        card: BleCardPayload,
    ) {
        if self.ble_handshake_session.is_some() {
            return;
        }
        let now = self.vauchi.clock().unix_seconds();
        let machine = match role {
            BleRole::Initiator => {
                BleHandshakeMachine::new_initiator(identity_key, identity_x3dh, card, now)
            }
            BleRole::Responder => {
                BleHandshakeMachine::new_responder(identity_key, identity_x3dh, card, now)
            }
        };
        self.ble_handshake_session = Some(BleHandshakeHolder { machine });
    }

    /// Cancel + drop the active machine. Drains the resulting
    /// `Command::BleDisconnect` (if any) into `pending_commands`.
    /// Idempotent.
    pub fn cancel_ble_handshake_session(&mut self) {
        if let Some(mut holder) = self.ble_handshake_session.take() {
            let cmds = holder.machine.cancel();
            if !cmds.is_empty() {
                self.extend_pending_commands(cmds);
            }
        }
    }

    /// Route a frontend-emitted [`vauchi_core::Event`] into the
    /// machine. Returns the resulting [`BleMachineEvent`] so the
    /// caller can flip engine chrome (e.g. `MobileBleState`
    /// transitions). Any commands the machine emits are drained
    /// into `pending_commands` automatically — the next
    /// `screen_envelope_to_json` surfaces them to the frontend.
    pub fn forward_ble_hardware_event(&mut self, event: &Event) -> BleMachineEvent {
        let Some(holder) = self.ble_handshake_session.as_mut() else {
            return BleMachineEvent::None;
        };
        let now = self.vauchi.clock().unix_seconds();
        let (m_event, cmds) = match event {
            Event::BleConnected { .. } => holder.machine.on_connected(now),
            Event::BleCharacteristicNotified { uuid, data } => {
                holder.machine.on_data_received(uuid, data, now)
            }
            Event::BleCharacteristicRead { uuid, data } => {
                // Frontends route a READ-response on the same UUID
                // surface as a notify; the machine's reassembler
                // handles both.
                holder.machine.on_data_received(uuid, data, now)
            }
            Event::BleMtuNegotiated { mtu, .. } => {
                holder.machine.update_mtu(*mtu);
                (BleMachineEvent::None, Vec::new())
            }
            Event::BleDisconnected { reason } => holder.machine.on_disconnected(reason),
            _ => (BleMachineEvent::None, Vec::new()),
        };
        if !cmds.is_empty() {
            self.extend_pending_commands(cmds);
        }
        m_event
    }

    /// Whether a BLE handshake machine is currently held.
    pub fn ble_handshake_session_active(&self) -> bool {
        self.ble_handshake_session.is_some()
    }

    /// Read the held machine's current phase, if any.
    pub fn ble_machine_phase(&self) -> Option<BleMachinePhase> {
        self.ble_handshake_session
            .as_ref()
            .map(|h| h.machine.phase())
    }
}
