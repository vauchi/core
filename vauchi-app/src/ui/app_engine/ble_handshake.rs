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

use super::{AppEngine, AppScreen};
use crate::orchestrator::ble_handshake_machine::{
    BleHandshakeMachine, BleMachineEvent, BleMachinePhase, BleRole, decide_ble_role,
};
use vauchi_core::Contact;
use vauchi_core::Event;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{DoubleRatchetState, X3DHKeyPair};
use vauchi_core::exchange::{BleCardPayload, BleExchangeResult};

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

    /// Derive this device's BLE handshake inputs from the live identity
    /// and own card: `(identity signing key, X3DH keypair, card
    /// payload)`. `None` when there is no identity yet (the caller skips
    /// session creation). The card's `exchange_key` must equal the
    /// returned keypair's public key, so both come from
    /// `identity.x3dh_keypair()` — documented to agree with
    /// `exchange_public_key`.
    fn build_ble_session_inputs(&self) -> Option<([u8; 32], X3DHKeyPair, BleCardPayload)> {
        let identity = self.vauchi.identity()?;
        let identity_key = *identity.signing_public_key();
        let x3dh = identity.x3dh_keypair();
        let exchange_pub = *x3dh.public_key();
        let display_name = identity.display_name().to_string();
        let card = self
            .vauchi
            .own_card()
            .ok()
            .flatten()
            .unwrap_or_else(|| ContactCard::new(&display_name));
        let fields = card
            .fields()
            .iter()
            .map(|f| (f.label().to_string(), f.value().to_string()))
            .collect();
        let avatar = card.avatar().map(|a| a.to_vec());
        let ble_card =
            BleCardPayload::new(identity_key, display_name, exchange_pub, fields, avatar);
        Some((identity_key, x3dh, ble_card))
    }

    /// Build the BLE handshake session when a peer is discovered. The
    /// role is decided from the symmetric tiebreak tokens — this
    /// device's identity signing key (the same value the engine
    /// advertises) vs the peer's advertised `peer_token` — via the
    /// shared [`decide_ble_role`], so the session role always agrees
    /// with `BleExchangeFlow`'s connect decision. Idempotent:
    /// `ensure_ble_handshake_session` is a no-op once a session is held,
    /// so re-discoveries never rebuild it. The subsequent
    /// `BleConnected` / data events drive the machine through
    /// `forward_ble_hardware_event`.
    pub fn start_ble_handshake_on_discovery(&mut self, peer_token: &[u8]) {
        if self.ble_handshake_session_active() {
            return;
        }
        let Some((identity_key, x3dh, card)) = self.build_ble_session_inputs() else {
            log::warn!("BLE: cannot start handshake — no identity / card");
            return;
        };
        let role = decide_ble_role(&identity_key, peer_token);
        self.ensure_ble_handshake_session(role, identity_key, x3dh, card);
    }

    /// Tear down the BLE handshake session when leaving the BLE exchange
    /// screen. The session is built lazily on discovery (its role is
    /// unknown at screen entry), so there is no entry branch — only
    /// teardown. Mirrors `sync_multi_stage_lifecycle`.
    pub(super) fn sync_ble_handshake_lifecycle(&mut self, old: &AppScreen, new: &AppScreen) {
        let was = matches!(old, AppScreen::BleExchange { .. });
        let is = matches!(new, AppScreen::BleExchange { .. });
        if was && !is {
            self.cancel_ble_handshake_session();
        }
    }

    /// Apply a [`BleMachineEvent`] returned by
    /// [`Self::forward_ble_hardware_event`]. On `Completed`, persist the
    /// decrypted peer card as an exchanged contact (with its Double
    /// Ratchet) so it appears in the contact list and future encrypted
    /// card updates from this peer decrypt. Returns `true` when a
    /// contact was created. Other events are inert here — engine chrome
    /// is driven separately.
    pub fn apply_ble_machine_event(&mut self, event: BleMachineEvent) -> bool {
        match event {
            BleMachineEvent::Completed(result) => {
                let persisted = self.persist_ble_exchanged_contact(&result);
                // Drive the chrome to its terminal Success screen. The
                // hollow `BleExchangeFlow` no longer self-completes from
                // BLE data bytes (P4), so the real machine's completion is
                // what flips the UI to success.
                if let Some(any) = self.engine.as_any_mut()
                    && let Some(active) = any.downcast_mut::<crate::ui::BleExchangeEngine>()
                {
                    active.force_success();
                }
                persisted
            }
            _ => false,
        }
    }

    /// Persist a completed BLE exchange: build the peer `Contact` from
    /// the decrypted `remote_card` + the handshake session key, store it,
    /// and save the role-correct Double Ratchet (parity with the
    /// multi-stage path — without it, future card updates from this peer
    /// would silently fail to decrypt). Best-effort: a missing session
    /// key / identity leaves the exchange complete on-screen but
    /// uncreated rather than panicking on the completion path. Returns
    /// `true` when the contact was added.
    fn persist_ble_exchanged_contact(&mut self, result: &BleExchangeResult) -> bool {
        // Clone the owned session key off the held machine under a short
        // borrow so the `self.vauchi` persistence calls below don't fight
        // the borrow.
        let Some(shared_key) = self
            .ble_handshake_session
            .as_ref()
            .and_then(|h| h.machine.session_key().cloned())
        else {
            log::warn!("BLE: completion without a session key — contact not created");
            return false;
        };
        let Some(identity) = self.vauchi.identity() else {
            return false;
        };
        let our_identity = *identity.signing_public_key();
        let our_x3dh = identity.x3dh_keypair();
        let their_identity = result.remote_card.identity_key;
        let their_exchange_key = result.remote_card.exchange_key;
        let now = self.vauchi.clock().unix_seconds();

        // Ratchet role convention matches
        // `ExchangeSession::build_exchange_ratchet`: the lexicographically
        // smaller identity is the ratchet initiator. This coincides with
        // the BLE connect tiebreak (the advertised token *is* the identity
        // signing key), but is derived independently from the identities
        // so the two stay correct even if the token source ever changes.
        let is_initiator = our_identity < their_identity;
        let ratchet = if is_initiator {
            match DoubleRatchetState::initialize_initiator(&shared_key, their_exchange_key) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("BLE: ratchet init (initiator) failed: {e:?}");
                    return false;
                }
            }
        } else {
            DoubleRatchetState::initialize_responder(&shared_key, our_x3dh)
        };

        let card = result.remote_card.to_contact_card(now);
        let contact = Contact::from_exchange_full(
            their_identity,
            card,
            shared_key,
            vauchi_core::types::ProximityConfidence::Unknown,
            vauchi_core::types::ExchangeTransport::Ble,
            now,
        );
        let contact_id = contact.id().to_string();
        if self.vauchi.add_contact(contact).is_err() {
            log::warn!("BLE: failed to add exchanged contact");
            return false;
        }
        if self
            .vauchi
            .save_exchange_ratchet(&contact_id, &ratchet, is_initiator)
            .is_err()
        {
            log::warn!("BLE: failed to persist exchange ratchet for {contact_id}");
        }
        true
    }
}
