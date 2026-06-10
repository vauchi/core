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
        // G2 privacy filter (shared chokepoint): share only the fields the
        // selected exchange group(s) may see. `pending_exchange_groups` is set
        // at the mode handoff (`result_routing.rs`) and consumed later at
        // completion, so it is read by reference here. `None` (no groups / no
        // own card) → fall back to a bare card.
        let card = crate::ui::exchange::group_filter::filtered_own_card(
            &self.vauchi,
            &self.pending_exchange_groups,
        )
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

    /// Build the BLE handshake session as the **responder** for a device
    /// acting as the GATT peripheral.
    ///
    /// The peripheral advertises and is connected *to* — it never scans, so
    /// it emits no `BleDeviceDiscovered` and `start_ble_handshake_on_discovery`
    /// is never reached. The peripheral that receives the KeyOffer is always
    /// the responder, so the role is fixed and no peer tiebreak token is
    /// needed (`process_key_offer` supplies the peer's keys). Idempotent: a
    /// no-op once a session is held, so the central — which already built its
    /// session on discovery before `BleConnected` — is unaffected.
    ///
    /// Without this the iOS peripheral-responder never builds the
    /// AppEngine-owned machine, so completion runs on the hollow chrome path
    /// and no contact is persisted
    /// (`2026-06-08-ios-ble-responder-persist`). Android scans even as the
    /// responder, so it built the session on discovery and was unaffected.
    pub fn start_ble_handshake_as_responder(&mut self) {
        if self.ble_handshake_session_active() {
            return;
        }
        let Some((identity_key, x3dh, card)) = self.build_ble_session_inputs() else {
            log::warn!("BLE: cannot start responder handshake — no identity / card");
            return;
        };
        self.ensure_ble_handshake_session(BleRole::Responder, identity_key, x3dh, card);
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
    /// contact was created. Terminal events also flip the engine chrome
    /// (the hollow flow observes no machine state); intermediate events
    /// are inert here.
    pub fn apply_ble_machine_event(&mut self, event: BleMachineEvent) -> bool {
        match event {
            BleMachineEvent::Completed(result) => {
                let persisted = self.persist_ble_exchanged_contact(&result);
                // Drive the chrome to its terminal Success screen. The
                // hollow `BleExchangeFlow` no longer self-completes from
                // BLE data bytes (P4), so the real machine's completion is
                // what flips the UI to success.
                if !self
                    .engine
                    .apply_update(crate::ui::EngineUpdate::BleForceSuccess)
                {
                    tracing::warn!("BleForceSuccess not consumed by active engine");
                }
                persisted
            }
            BleMachineEvent::Failed { reason } => {
                // Machine-level failure (crypto / protocol error) has no
                // hardware event for the hollow flow to observe — flip the
                // chrome to Failed here or the UI shows "Exchanging..."
                // forever (P5b re-test, 2026-06-10).
                if let Some(any) = self.engine.as_any_mut()
                    && let Some(active) = any.downcast_mut::<crate::ui::BleExchangeEngine>()
                {
                    active.force_failure(Some(reason));
                }
                false
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
        // G4: file the new contact into the groups chosen in the exchange
        // preamble (best-effort — a group failure doesn't undo the
        // exchange). Same `pending_exchange_groups` carry the multi-stage
        // path uses; populated by `start_exchange_to` on mode dispatch.
        let pending_groups = std::mem::take(&mut self.pending_exchange_groups);
        for group_id in &pending_groups {
            // Best-effort; bind (not `let _`) to satisfy the must-use lint.
            let _added = self.vauchi.add_contact_to_group(group_id, &contact_id);
        }
        // Snapshot what this contact can now see as the revocation baseline
        // (2026-06-08-card-revocation-not-propagated). Best-effort.
        let _baseline = self.vauchi.initialize_sent_baseline(&contact_id);
        // Capture-at-exchange (ADR-051): BLE (Magic/Bump/Shake) is an in-person
        // mode, so record where this contact was met — same seam as the
        // multi-stage path. The Event::LocationResult reply is consumed in
        // handle_hardware_event.
        self.request_exchange_location(contact_id.clone());
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

// INLINE_TEST_REQUIRED: tests call the private `build_ble_session_inputs`
// and set the private `pending_exchange_groups` field — neither is reachable
// from a `tests/` integration directory.
#[cfg(test)]
mod tests {
    use super::*;
    use vauchi_core::api::Vauchi;
    use vauchi_core::contact_card::{ContactField, FieldType};

    /// AppEngine over an in-memory Vauchi whose own card carries `Email` +
    /// `Phone`, plus a "Work" group exposing only `Email`. Returns the engine
    /// and the Work group id.
    fn engine_with_card_and_group() -> (AppEngine, String) {
        let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
        vauchi.create_identity("Alice").expect("identity");
        let mut card = vauchi
            .own_card()
            .expect("own_card")
            .expect("create_identity saves a card");
        card.add_field(ContactField::new(FieldType::Email, "Email", "a@b.com", 0))
            .expect("add email");
        card.add_field(ContactField::new(
            FieldType::Phone,
            "Phone",
            "+12025550123",
            0,
        ))
        .expect("add phone");
        vauchi.update_own_card(&card).expect("update own card");
        let email_id = card
            .fields()
            .iter()
            .find(|f| f.label() == "Email")
            .expect("email field")
            .id()
            .to_string();
        let work = vauchi.create_group("Work").expect("create group");
        let work_id = work.id().to_string();
        vauchi
            .set_group_field_visibility(&work_id, &email_id, true)
            .expect("expose email to Work");
        (AppEngine::new(vauchi), work_id)
    }

    fn payload_labels(engine: &AppEngine) -> Vec<String> {
        let (_id, _x3dh, card) = engine
            .build_ble_session_inputs()
            .expect("identity + card present");
        card.fields.iter().map(|(label, _)| label.clone()).collect()
    }

    // @internal
    #[test]
    fn ble_payload_shares_full_card_when_no_group_selected() {
        // pending_exchange_groups empty → resolver returns None → share all.
        let (engine, _work) = engine_with_card_and_group();
        let labels = payload_labels(&engine);
        assert!(labels.contains(&"Email".to_string()), "Email shared");
        assert!(
            labels.contains(&"Phone".to_string()),
            "no group selected → full card (Phone shared)"
        );
    }

    // @internal
    #[test]
    fn ble_payload_filtered_to_selected_group_visible_fields() {
        // Work exposes only Email; selecting it must drop Phone from the
        // transmitted BLE payload (the privacy fix).
        let (mut engine, work) = engine_with_card_and_group();
        engine.pending_exchange_groups = vec![work];
        let labels = payload_labels(&engine);
        assert_eq!(
            labels,
            vec!["Email".to_string()],
            "Work group exposes only Email; Phone must not be transmitted"
        );
    }

    // @internal
    #[test]
    fn ble_payload_empty_when_selected_group_exposes_nothing() {
        // Default-closed: a selected group with no visible_fields shares no
        // fields (Some(∅)), NOT the full card.
        let (mut engine, _work) = engine_with_card_and_group();
        let empty = engine
            .vauchi
            .create_group("Empty")
            .expect("create empty group");
        engine.pending_exchange_groups = vec![empty.id().to_string()];
        let labels = payload_labels(&engine);
        assert!(
            labels.is_empty(),
            "empty group → share nothing, got {labels:?}"
        );
    }

    /// Route one side's pending BLE writes to the other as notifications on
    /// the same characteristic (a GATT write on uuid X surfaces at the peer
    /// as data on uuid X), applying any resulting machine event (which
    /// persists the contact on `Completed`). Returns the writes routed.
    fn pump(from: &mut AppEngine, to: &mut AppEngine) -> usize {
        let mut routed = 0;
        for cmd in from.drain_pending_commands() {
            if let vauchi_core::Command::BleWriteCharacteristic { uuid, data } = cmd {
                routed += 1;
                let ev =
                    to.forward_ble_hardware_event(&vauchi_core::Event::BleCharacteristicNotified {
                        uuid,
                        data,
                    });
                to.apply_ble_machine_event(ev);
            }
        }
        routed
    }

    // @internal
    #[test]
    fn two_device_ble_exchange_peer_receives_only_group_visible_fields() {
        // End-to-end G4 ratchet: Alice shares to a Work group exposing only
        // Email; after a full two-device BLE exchange Bob's stored contact
        // card must carry Email and NOT Phone — the privacy guarantee.
        let (mut alice, work) = engine_with_card_and_group();
        alice.pending_exchange_groups = vec![work];

        let mut vauchi_bob = Vauchi::in_memory().expect("in-memory vauchi");
        vauchi_bob.create_identity("Bob").expect("identity");
        let mut bob = AppEngine::new(vauchi_bob);

        let alice_token = alice
            .vauchi
            .identity()
            .expect("alice identity")
            .signing_public_key()
            .to_vec();
        let bob_token = bob
            .vauchi
            .identity()
            .expect("bob identity")
            .signing_public_key()
            .to_vec();

        // Each discovers the other → builds a session with the tiebreak role.
        alice.start_ble_handshake_on_discovery(&bob_token);
        bob.start_ble_handshake_on_discovery(&alice_token);

        // Connect both; the initiator emits its KeyOffer on connect.
        let ea = alice.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
            device_id: "bob".into(),
        });
        alice.apply_ble_machine_event(ea);
        let eb = bob.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
            device_id: "alice".into(),
        });
        bob.apply_ble_machine_event(eb);

        // Pump writes back and forth until the exchange settles.
        for _ in 0..50 {
            let a = pump(&mut alice, &mut bob);
            let b = pump(&mut bob, &mut alice);
            if a + b == 0 {
                break;
            }
        }

        let bob_contacts = bob.vauchi.list_contacts().expect("list contacts");
        assert_eq!(bob_contacts.len(), 1, "Bob should have exactly Alice");
        let alice_card = bob_contacts[0].card();
        let labels: Vec<&str> = alice_card.fields().iter().map(|f| f.label()).collect();
        assert!(
            labels.contains(&"Email"),
            "Email is in the Work group → must reach Bob; got {labels:?}"
        );
        assert!(
            !labels.contains(&"Phone"),
            "Phone is NOT in the Work group → must NOT reach Bob; got {labels:?}"
        );
    }

    // @scenario: ble_exchange :: Both peers persist the exchanged contact
    #[test]
    fn two_device_ble_exchange_persists_contact_for_both_roles() {
        // Regression guard for the iOS responder-persist bug
        // (2026-06-08-ios-ble-responder-persist): the responder reached
        // "Completed" but created no contact. Persistence is core-driven and
        // role-symmetric — BOTH the handshake initiator and responder must
        // create the peer contact. The role is decided by the identity
        // tiebreak, so asserting only one side (as the privacy test above
        // does) covers the responder path only ~half the time. Assert both:
        // whichever engine is the responder, its persist must succeed (a live
        // session key at completion).
        let mut va = Vauchi::in_memory().expect("vauchi alice");
        va.create_identity("Alice").expect("alice identity");
        let mut alice = AppEngine::new(va);

        let mut vb = Vauchi::in_memory().expect("vauchi bob");
        vb.create_identity("Bob").expect("bob identity");
        let mut bob = AppEngine::new(vb);

        let alice_token = alice
            .vauchi
            .identity()
            .expect("alice identity")
            .signing_public_key()
            .to_vec();
        let bob_token = bob
            .vauchi
            .identity()
            .expect("bob identity")
            .signing_public_key()
            .to_vec();

        alice.start_ble_handshake_on_discovery(&bob_token);
        bob.start_ble_handshake_on_discovery(&alice_token);

        let ea = alice.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
            device_id: "bob".into(),
        });
        alice.apply_ble_machine_event(ea);
        let eb = bob.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
            device_id: "alice".into(),
        });
        bob.apply_ble_machine_event(eb);

        for _ in 0..50 {
            let a = pump(&mut alice, &mut bob);
            let b = pump(&mut bob, &mut alice);
            if a + b == 0 {
                break;
            }
        }

        assert_eq!(
            alice.vauchi.list_contacts().expect("alice contacts").len(),
            1,
            "Alice must persist Bob after completion — 0 means the role she \
             played (initiator or responder) failed to persist"
        );
        assert_eq!(
            bob.vauchi.list_contacts().expect("bob contacts").len(),
            1,
            "Bob must persist Alice after completion — 0 means the role he \
             played (initiator or responder) failed to persist"
        );
    }

    // @scenario: ble_exchange :: A peripheral responder (no discovery) persists the contact
    #[test]
    fn responder_built_on_connect_without_discovery_persists() {
        // Reproduces the iOS peripheral-responder path
        // (2026-06-08-ios-ble-responder-persist): the peripheral never emits
        // `BleDeviceDiscovered`, so it builds its session via
        // `start_ble_handshake_as_responder` (driven from `BleConnected` in
        // the platform layer) instead of `start_ble_handshake_on_discovery`.
        // It must still decrypt the peer card and persist the contact.
        let (mut initiator, _work) = engine_with_card_and_group();
        // Force the initiator role deterministically (the central would have
        // built this on discovery via the tiebreak).
        let (ik, x3dh, card) = initiator
            .build_ble_session_inputs()
            .expect("initiator inputs");
        initiator.ensure_ble_handshake_session(BleRole::Initiator, ik, x3dh, card);

        let mut vb = Vauchi::in_memory().expect("vauchi bob");
        vb.create_identity("Bob").expect("bob identity");
        let mut responder = AppEngine::new(vb);
        // The peripheral path: no discovery happened, so build as responder.
        responder.start_ble_handshake_as_responder();
        assert!(
            responder.ble_handshake_session_active(),
            "responder session must build without a prior discovery"
        );

        let ei = initiator.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
            device_id: "bob".into(),
        });
        initiator.apply_ble_machine_event(ei);
        let er = responder.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
            device_id: "alice".into(),
        });
        responder.apply_ble_machine_event(er);

        for _ in 0..50 {
            let a = pump(&mut initiator, &mut responder);
            let b = pump(&mut responder, &mut initiator);
            if a + b == 0 {
                break;
            }
        }

        assert_eq!(
            responder.vauchi.list_contacts().expect("contacts").len(),
            1,
            "a peripheral responder built on connect (no discovery) must \
             persist the peer contact"
        );
    }
}
