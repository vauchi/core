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
    BleHandshakeMachine, BleMachineEvent, BleMachinePhase, BleOobBinding, BleRole, decide_ble_role,
};
use vauchi_core::Contact;
use vauchi_core::Event;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::X3DHKeyPair;
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
    ///
    /// `oob` carries the Glance one-sided-QR binding
    /// (2026-06-10-ble-unauthenticated-peer-identity, Tier 1 Slice B): the
    /// scanner/initiator supplies `expected_peer` + `oob_nonce_echo`, the
    /// displayer/responder supplies `required_oob_nonce`. Radio-only modes
    /// (Magic/Bump/Shake) pass `None` — no OOB peer.
    pub fn ensure_ble_handshake_session(
        &mut self,
        role: BleRole,
        identity_key: [u8; 32],
        identity_x3dh: X3DHKeyPair,
        card: BleCardPayload,
        oob: Option<BleOobBinding>,
    ) {
        if self.ble_handshake_session.is_some() {
            return;
        }
        let now = self.vauchi.clock().unix_seconds();
        let machine = match role {
            BleRole::Initiator => {
                BleHandshakeMachine::new_initiator(identity_key, identity_x3dh, card, now, oob)
            }
            BleRole::Responder => {
                BleHandshakeMachine::new_responder(identity_key, identity_x3dh, card, now, oob)
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
            Event::BleConnected {
                device_id,
                direction,
            } => holder.machine.on_connected(device_id, *direction, now),
            Event::BleCharacteristicNotified {
                device_id,
                uuid,
                data,
            } => holder.machine.on_data_received(device_id, uuid, data, now),
            Event::BleCharacteristicRead {
                device_id,
                uuid,
                data,
            } => {
                // Frontends route a READ-response on the same UUID
                // surface as a notify; the machine's reassembler
                // handles both.
                holder.machine.on_data_received(device_id, uuid, data, now)
            }
            Event::BleMtuNegotiated { mtu, .. } => {
                holder.machine.update_mtu(*mtu);
                (BleMachineEvent::None, Vec::new())
            }
            Event::BleDisconnected {
                device_id,
                direction,
                reason,
            } => holder
                .machine
                .on_disconnected(device_id, *direction, reason),
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

    /// Whether an addressed `BleDisconnected` names a link OTHER than the
    /// one the active, non-terminal handshake machine rides — i.e. the
    /// deliberately torn-down glare loser. Such a disconnect is cleanup,
    /// not a session failure, and must not tear the exchange flow down.
    /// Un-addressed disconnects (empty id) are wildcards: never survived.
    pub(crate) fn ble_machine_survives_disconnect(
        &self,
        device_id: &str,
        direction: vauchi_core::BleLinkDirection,
    ) -> bool {
        let Some(holder) = self.ble_handshake_session.as_ref() else {
            return false;
        };
        if holder.machine.is_terminal() {
            return false;
        }
        match holder.machine.active_link() {
            Some((active, active_dir)) if !active.is_empty() && !device_id.is_empty() => {
                active != device_id || active_dir != direction
            }
            _ => false,
        }
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
            tracing::warn!("BLE: cannot start handshake — no identity / card");
            return;
        };
        let role = decide_ble_role(&identity_key, peer_token);
        tracing::info!("[Exchange] BLE peer discovered — starting handshake");
        // `None`: radio-only discovery has no OOB peer. The Glance scanner path
        // (which forces Initiator + supplies the scanned binding) lands in the
        // engine/orchestration step of Slice B.
        self.ensure_ble_handshake_session(role, identity_key, x3dh, card, None);
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
            tracing::warn!("BLE: cannot start responder handshake — no identity / card");
            return;
        };
        // Glance displayer: require the nonce this device showed in its QR, so
        // a connector that never scanned it is rejected (`OobNonceMismatch`).
        // Radio-only modes never called `begin_glance_display`, so the nonce is
        // `None` and this is the unchanged radio-responder path.
        let oob = self.glance_display_nonce.map(|nonce| BleOobBinding {
            required_oob_nonce: Some(nonce),
            ..Default::default()
        });
        self.ensure_ble_handshake_session(BleRole::Responder, identity_key, x3dh, card, oob);
    }

    /// Begin the Glance one-sided-QR display: build this device's OOB bootstrap
    /// payload and remember the nonce it must require as the responder. Returns
    /// the base64 payload for a `Component::QrCode`. The QR's exchange key is
    /// the identity's X3DH public key — the same value the handshake feeds into
    /// the DH — so the scanner's exchange-key pin accepts the honest peer (a
    /// fresh ephemeral would be rejected).
    pub fn begin_glance_display(&mut self) -> Option<String> {
        let now = self.vauchi.clock().unix_seconds();
        let qr = {
            let identity = self.vauchi.identity()?;
            let ephemeral = identity.x3dh_keypair();
            vauchi_core::exchange::oob_bootstrap::OobBootstrapQr::generate(
                identity, &ephemeral, now,
            )
        };
        self.glance_display_nonce = Some(qr.oob_nonce());
        Some(qr.to_data_string())
    }

    /// Apply a scanned Glance QR: verify it (signature + expiry), latch this
    /// device into the scanner role, and pin the displayer's identity +
    /// exchange key + co-presence nonce. A tampered or expired QR returns an
    /// error and latches nothing.
    pub fn apply_glance_scan(&mut self, data: &str) -> Result<(), vauchi_core::ExchangeError> {
        let now = self.vauchi.clock().unix_seconds();
        let qr = vauchi_core::exchange::oob_bootstrap::OobBootstrapQr::verified_from_data_string(
            data, now,
        )?;
        self.glance_scanned = Some(BleOobBinding {
            expected_peer: Some(*qr.identity_key()),
            expected_exchange_key: Some(*qr.exchange_key()),
            oob_nonce_echo: Some(qr.oob_nonce()),
            required_oob_nonce: None,
        });
        Ok(())
    }

    /// Scanner-side discovery gate for Glance: connect to a discovered device
    /// ONLY if its advertised identity is the one this device scanned. Builds
    /// the initiator session with the scanned pins and drains a
    /// `Command::BleConnect`. A no-op for a non-scanner or a non-matching
    /// advertiser — asymmetric discovery, no tiebreak, no latch race.
    pub fn handle_glance_discovery(&mut self, device_id: &str, adv_data: &[u8]) {
        let Some(binding) = self.glance_scanned else {
            return; // not the scanner — this device waits to be connected to
        };
        let Some(expected) = binding.expected_peer else {
            return;
        };
        if adv_data != &expected[..] {
            return; // an advertiser we did not scan — ignore it
        }
        if self.ble_handshake_session_active() {
            return;
        }
        let Some((identity_key, x3dh, card)) = self.build_ble_session_inputs() else {
            tracing::warn!("BLE: cannot start Glance scanner handshake — no identity / card");
            return;
        };
        self.ensure_ble_handshake_session(
            BleRole::Initiator,
            identity_key,
            x3dh,
            card,
            Some(binding),
        );
        self.extend_pending_commands(vec![vauchi_core::Command::BleConnect {
            device_id: device_id.to_string(),
        }]);
    }

    /// Tear down BLE-exchange state on leaving the screen. The handshake
    /// session is built lazily on discovery/connect (its role is unknown at
    /// entry), so there is no session entry branch here; the Glance one-sided
    /// QR is generated pre-engine-build in `navigate_to`. On exit the whole
    /// Glance OOB state is cleared so it cannot leak into the next exchange.
    /// Mirrors `sync_multi_stage_lifecycle`.
    pub(super) fn sync_ble_handshake_lifecycle(&mut self, old: &AppScreen, new: &AppScreen) {
        let was = matches!(old, AppScreen::BleExchange { .. });
        let is = matches!(new, AppScreen::BleExchange { .. });
        if was && !is {
            self.cancel_ble_handshake_session();
            self.glance_display_qr = None;
            self.glance_display_nonce = None;
            self.glance_scanned = None;
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
                // G1: emit the reciprocity ack ONLY AFTER the contact persisted,
                // so "peer got my token ⇒ I persisted" (design P1). Inert on the
                // peer until its receive-side handler lands (step 2b).
                if persisted.is_some() {
                    let ack_cmd = self
                        .ble_handshake_session
                        .as_ref()
                        .and_then(|h| h.machine.build_reciprocity_ack_command());
                    if let Some(cmd) = ack_cmd {
                        self.extend_pending_commands(vec![cmd]);
                    }
                    // Ceremony (M2 S4): validated + persisted success — once.
                    self.extend_pending_commands(vec![
                        crate::ui::exchange::ceremony::exchange_celebrate(),
                    ]);
                }
                // Drive the chrome to its terminal Success screen with the
                // rich summary (M2 S6) — who you met + what they shared. The
                // hollow `BleExchangeFlow` no longer self-completes from
                // BLE data bytes (P4), so the real machine's completion is
                // what flips the UI to success.
                let summary = persisted.as_ref().map(|(contact_id, group_names)| {
                    Box::new(self.build_exchange_summary(contact_id, group_names.clone()))
                });
                if !self
                    .engine
                    .apply_update(crate::ui::EngineUpdate::BleForceSuccess { summary })
                {
                    tracing::warn!("BleForceSuccess not consumed by active engine");
                }
                persisted.is_some()
            }
            BleMachineEvent::Failed { reason } => {
                // Machine-level failure (crypto / protocol error) has no
                // hardware event for the hollow flow to observe — flip the
                // chrome to Failed here or the UI shows "Exchanging..."
                // forever (P5b re-test, 2026-06-10).
                if !self
                    .engine
                    .apply_update(crate::ui::EngineUpdate::BleForceFailure {
                        reason: Some(reason),
                    })
                {
                    tracing::warn!("BleForceFailure not consumed by active engine");
                }
                false
            }
            BleMachineEvent::ReciprocityConfirmed { their_identity } => {
                // P1 step 2b: the peer's ack verified — flip the contact to
                // Confirmed (banner clears). Best-effort.
                let _confirmed = self.vauchi.confirm_contact_reciprocity(&their_identity);
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
    fn persist_ble_exchanged_contact(
        &mut self,
        result: &BleExchangeResult,
    ) -> Option<(String, Vec<String>)> {
        // Clone the owned session key off the held machine under a short
        // borrow so the `self.vauchi` persistence calls below don't fight
        // the borrow.
        let Some(shared_key) = self
            .ble_handshake_session
            .as_ref()
            .and_then(|h| h.machine.session_key().cloned())
        else {
            tracing::warn!("BLE: completion without a session key — contact not created");
            return None;
        };
        let identity = self.vauchi.identity()?;
        let our_identity = *identity.signing_public_key();
        let our_x3dh = identity.x3dh_keypair();
        let their_identity = result.remote_card.identity_key;
        let their_exchange_key = result.remote_card.exchange_key;
        let now = self.vauchi.clock().unix_seconds();

        // Role + init via the shared seam (consolidation Step 2a): the
        // helper owns the smaller-identity-initiates rule and the
        // role-correct key selection, so BLE can no longer drift from the
        // QR/multi-stage sessions. Equivalence + interop pinned by
        // `consolidation_pinning_tests` and the interop test below.
        let (ratchet, is_initiator) =
            match vauchi_core::exchange::ratchet_bootstrap::bootstrap_exchange_ratchet(
                &shared_key,
                &our_identity,
                &their_identity,
                Some(their_exchange_key),
                Some(our_x3dh),
            ) {
                Ok(pair) => pair,
                Err(e) => {
                    tracing::warn!("BLE: ratchet init failed: {e:?}");
                    return None;
                }
            };

        let card = result.remote_card.to_contact_card(now);
        let mut contact = Contact::from_exchange_full(
            their_identity,
            card,
            shared_key,
            vauchi_core::types::ProximityConfidence::Unknown,
            vauchi_core::types::ExchangeTransport::Ble,
            now,
        );
        // Confirmable but unconfirmed until the peer's ack — record Pending so
        // it surfaces via the banner (2026-06-04-exchange-terminal-screens).
        contact.set_reciprocity(vauchi_core::exchange::reciprocity::Reciprocity::Pending);
        let contact_id = contact.id().to_string();
        // Upsert + rekey via the unified core routine so a REPEAT BLE exchange
        // updates the peer's card and rekeys, instead of silently dropping it:
        // the old `add_contact` rejected the duplicate id and returned before
        // the ratchet was saved. Repeat-exchange decision 2026-06-27.
        if let Err(e) = self
            .vauchi
            .save_exchanged_contact(&contact, &ratchet, is_initiator)
        {
            tracing::warn!("BLE: failed to persist exchanged contact/ratchet: {e}");
            return None;
        }
        // G4: file the new contact into the groups chosen in the exchange
        // preamble (best-effort — a group failure doesn't undo the
        // exchange). Same `pending_exchange_groups` carry the multi-stage
        // path uses; populated by `start_exchange_to` on mode dispatch.
        let pending_groups = std::mem::take(&mut self.pending_exchange_groups);
        let mut group_names = Vec::new();
        for group_id in &pending_groups {
            // Best-effort; names collected for the rich success summary
            // (M2 S6), mirroring the multi-stage persist.
            if self
                .vauchi
                .add_contact_to_group(group_id, &contact_id)
                .is_ok()
                && let Ok(group) = self.vauchi.get_group(group_id)
            {
                group_names.push(group.name().to_string());
            }
        }
        // Snapshot what this contact can now see as the revocation baseline
        // (2026-06-08-card-revocation-not-propagated). Best-effort.
        let _baseline = self.vauchi.initialize_sent_baseline(&contact_id);
        // Capture-at-exchange (ADR-051): BLE (Magic/Bump/Shake) is an in-person
        // mode, so record where this contact was met — same seam as the
        // multi-stage path. The Event::LocationResult reply is consumed in
        // handle_hardware_event.
        self.request_exchange_location(contact_id.clone());
        tracing::info!("[Exchange] contact persisted (ble)");
        Some((contact_id, group_names))
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
    use vauchi_core::platform::BleLinkDirection;

    /// AppEngine over an in-memory Vauchi whose own card carries `Email` +
    /// `Phone`, plus a "Work" group exposing only `Email`. Returns the engine
    /// and the Work group id.
    /// Resolves an own-card field label to its generated id.
    fn own_field_id(vauchi: &Vauchi, label: &str) -> String {
        let card = vauchi.own_card().expect("own_card").expect("card present");
        let field = card.fields().iter().find(|f| f.label() == label);
        field.expect("labeled field").id().to_string()
    }

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
        let email_id = own_field_id(&vauchi, "Email");
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
    fn ble_payload_shares_visible_toggled_base_when_no_group_selected() {
        // No selection → curated base: the Visible-toggled unassigned Phone
        // ships; Work-assigned Email stays out (field-centric, 2026-07-10).
        let (engine, _work) = engine_with_card_and_group();
        engine
            .vauchi
            .set_own_field_public(&own_field_id(&engine.vauchi, "Phone"))
            .expect("toggle Phone visible");
        assert_eq!(payload_labels(&engine), vec!["Phone".to_string()]);
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
            if let vauchi_core::Command::BleWriteCharacteristic {
                device_id: _,
                uuid,
                data,
            } = cmd
            {
                routed += 1;
                // Forward un-addressed (wildcard): the writer stamps ITS
                // link id, which names the receiver from the writer's side;
                // a real shell re-stamps with the receiver-side link id.
                // Link scoping has dedicated machine-level tests.
                let ev =
                    to.forward_ble_hardware_event(&vauchi_core::Event::BleCharacteristicNotified {
                        device_id: String::new(),
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
            direction: BleLinkDirection::Outbound,
        });
        alice.apply_ble_machine_event(ea);
        let eb = bob.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
            device_id: "alice".into(),
            direction: BleLinkDirection::Inbound,
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
            direction: BleLinkDirection::Outbound,
        });
        alice.apply_ble_machine_event(ea);
        let eb = bob.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
            device_id: "alice".into(),
            direction: BleLinkDirection::Inbound,
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

    // @scenario: ble_exchange :: Persisted ratchets from both roles form a working channel
    #[test]
    fn two_device_ble_exchange_persisted_ratchets_interoperate() {
        // Consolidation Step-1 pin (U3): the BLE persist path hand-rolls
        // its ratchet init instead of calling
        // `ratchet_bootstrap::bootstrap_exchange_ratchet`. This drives the
        // REAL two-device flow and proves the two persisted states form a
        // working bidirectional channel with complementary roles — the
        // contract the planned helper substitution must preserve. Recipe
        // equivalence itself is pinned in core
        // `tests/it/consolidation_pinning_tests.rs`.
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
            direction: BleLinkDirection::Outbound,
        });
        alice.apply_ble_machine_event(ea);
        let eb = bob.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
            device_id: "alice".into(),
            direction: BleLinkDirection::Inbound,
        });
        bob.apply_ble_machine_event(eb);

        for _ in 0..50 {
            let a = pump(&mut alice, &mut bob);
            let b = pump(&mut bob, &mut alice);
            if a + b == 0 {
                break;
            }
        }

        let alices_bob = alice.vauchi.list_contacts().expect("alice contacts");
        let bobs_alice = bob.vauchi.list_contacts().expect("bob contacts");
        assert_eq!(alices_bob.len(), 1, "alice persisted bob");
        assert_eq!(bobs_alice.len(), 1, "bob persisted alice");

        let (ra, a_init) = alice
            .vauchi
            .storage()
            .ratchets()
            .load_ratchet_state(alices_bob[0].id())
            .expect("alice load ok")
            .expect("alice persisted a ratchet");
        let (rb, b_init) = bob
            .vauchi
            .storage()
            .ratchets()
            .load_ratchet_state(bobs_alice[0].id())
            .expect("bob load ok")
            .expect("bob persisted a ratchet");
        assert_ne!(a_init, b_init, "exactly one side is the ratchet initiator");
        // Shape-(a) characterization (consolidation Step 1): BLE persists
        // via `from_exchange_full` + `save_exchanged_contact` — transport
        // stamped `Ble`, role flag equal to the canonical smaller-identity
        // rule the shared helper encodes.
        assert_eq!(
            alices_bob[0].exchange_transport(),
            Some(vauchi_core::types::ExchangeTransport::Ble),
            "BLE persist stamps its transport"
        );
        assert_eq!(
            a_init,
            alice_token < bob_token,
            "persisted role flag matches the canonical smaller-identity rule"
        );

        let (mut init_side, mut resp_side) = if a_init { (ra, rb) } else { (rb, ra) };
        let m1 = init_side.encrypt(b"probe-1").expect("initiator encrypts");
        assert_eq!(
            resp_side.decrypt(&m1).expect("responder decrypts"),
            b"probe-1".to_vec()
        );
        let m2 = resp_side.encrypt(b"probe-2").expect("responder replies");
        assert_eq!(
            init_side.decrypt(&m2).expect("initiator decrypts"),
            b"probe-2".to_vec()
        );
        let m3 = init_side
            .encrypt(b"probe-3")
            .expect("initiator crosses ratchet step");
        assert_eq!(
            resp_side.decrypt(&m3).expect("responder decrypts m3"),
            b"probe-3".to_vec()
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
        initiator.ensure_ble_handshake_session(BleRole::Initiator, ik, x3dh, card, None);

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
            direction: BleLinkDirection::Outbound,
        });
        initiator.apply_ble_machine_event(ei);
        let er = responder.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
            device_id: "alice".into(),
            direction: BleLinkDirection::Inbound,
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

    // ============================================================
    // Glance Slice B — OOB binding supply (pin + nonce echo) reaches the
    // session enforcement. The binding is the exposure-closer for
    // 2026-06-10-ble-unauthenticated-peer-identity.
    // ============================================================

    fn fresh_engine(name: &str) -> AppEngine {
        let mut v = Vauchi::in_memory().expect("in-memory vauchi");
        v.create_identity(name).expect("identity");
        AppEngine::new(v)
    }

    fn signing_key(e: &AppEngine) -> [u8; 32] {
        *e.vauchi.identity().expect("identity").signing_public_key()
    }

    fn ensure_with_oob(e: &mut AppEngine, role: BleRole, oob: Option<BleOobBinding>) {
        let (ik, x3dh, card) = e.build_ble_session_inputs().expect("session inputs");
        e.ensure_ble_handshake_session(role, ik, x3dh, card, oob);
    }

    fn run_handshake(initiator: &mut AppEngine, responder: &mut AppEngine) {
        let ei = initiator.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
            device_id: "responder".into(),
            direction: BleLinkDirection::Outbound,
        });
        initiator.apply_ble_machine_event(ei);
        let er = responder.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
            device_id: "initiator".into(),
            direction: BleLinkDirection::Inbound,
        });
        responder.apply_ble_machine_event(er);
        for _ in 0..50 {
            let a = pump(initiator, responder);
            let b = pump(responder, initiator);
            if a + b == 0 {
                break;
            }
        }
    }

    // @scenario: ble_exchange :: Glance scanner rejects a foreign displayer
    #[test]
    fn glance_scanner_rejects_foreign_displayer_via_identity_pin() {
        // The scanner scanned the REAL displayer's QR and pinned its identity.
        // A radio-range MITM (Mallory) answers instead — she even knows the
        // co-presence nonce (worst case: the QR was shoulder-surfed) but not
        // the pinned identity's signing key, so the scanner must reject her.
        let mut scanner = fresh_engine("Scanner");
        let displayer = fresh_engine("RealDisplayer");
        let mut mallory = fresh_engine("Mallory");

        let pinned = signing_key(&displayer);
        let nonce = [5u8; 16];

        ensure_with_oob(
            &mut scanner,
            BleRole::Initiator,
            Some(BleOobBinding {
                expected_peer: Some(pinned),
                oob_nonce_echo: Some(nonce),
                ..Default::default()
            }),
        );
        ensure_with_oob(
            &mut mallory,
            BleRole::Responder,
            Some(BleOobBinding {
                required_oob_nonce: Some(nonce),
                ..Default::default()
            }),
        );

        run_handshake(&mut scanner, &mut mallory);

        assert_eq!(
            scanner.vauchi.list_contacts().expect("contacts").len(),
            0,
            "scanner pinned the scanned identity — Mallory's mismatched \
             identity must abort the handshake (no contact persisted)"
        );
    }

    // @scenario: ble_exchange :: Glance displayer rejects a connector that never scanned
    #[test]
    fn glance_displayer_rejects_connector_without_nonce_echo() {
        // The displayer requires the co-presence nonce it showed in its QR. A
        // connector that never scanned it (no echo) must be rejected — this is
        // what stops a non-co-present device from harvesting the displayer's
        // card by merely winning the radio race.
        let mut displayer = fresh_engine("Displayer");
        let mut harvester = fresh_engine("Harvester");
        let nonce = [9u8; 16];

        ensure_with_oob(
            &mut displayer,
            BleRole::Responder,
            Some(BleOobBinding {
                required_oob_nonce: Some(nonce),
                ..Default::default()
            }),
        );
        ensure_with_oob(&mut harvester, BleRole::Initiator, None);

        run_handshake(&mut harvester, &mut displayer);

        assert_eq!(
            displayer.vauchi.list_contacts().expect("contacts").len(),
            0,
            "displayer requires the QR nonce — a connector without the echo \
             must be rejected (no contact persisted)"
        );
    }

    // @scenario: ble_exchange :: Glance matching binding completes for both peers
    #[test]
    fn glance_matching_binding_completes_and_persists() {
        // Happy path: the scanner echoes the displayer's nonce and pins its
        // identity; the displayer requires that nonce. Both checks pass, the
        // exchange completes, and both persist the peer.
        let mut scanner = fresh_engine("Scanner");
        let mut displayer = fresh_engine("Displayer");
        let pinned = signing_key(&displayer);
        let nonce = [7u8; 16];

        ensure_with_oob(
            &mut scanner,
            BleRole::Initiator,
            Some(BleOobBinding {
                expected_peer: Some(pinned),
                oob_nonce_echo: Some(nonce),
                ..Default::default()
            }),
        );
        ensure_with_oob(
            &mut displayer,
            BleRole::Responder,
            Some(BleOobBinding {
                required_oob_nonce: Some(nonce),
                ..Default::default()
            }),
        );

        run_handshake(&mut scanner, &mut displayer);

        assert_eq!(
            scanner.vauchi.list_contacts().expect("contacts").len(),
            1,
            "scanner must persist the pinned displayer on success"
        );
        assert_eq!(
            displayer.vauchi.list_contacts().expect("contacts").len(),
            1,
            "displayer must persist the co-present scanner on success"
        );
    }

    // ============================================================
    // Glance orchestration — scan → binding → gated discovery. The AppEngine
    // computes the BleOobBinding from live QR state (the layer above the
    // binding-threading tests: those inject the binding directly).
    // ============================================================

    // @scenario: ble_exchange :: Glance symmetric one-sided-QR completes for both peers
    #[test]
    fn glance_orchestration_symmetric_happy_path_both_persist() {
        // Symmetric UX: both devices display a QR + advertise + scan. Bob scans
        // Alice's QR (latching scanner), discovers Alice advertising, connects;
        // Alice (peripheral) responds. The pins are computed from the QR — no
        // binding is injected by hand.
        let mut alice = fresh_engine("Alice"); // displayer/responder
        let mut bob = fresh_engine("Bob"); // scanner/initiator

        let alice_qr = alice.begin_glance_display().expect("alice shows a QR");
        let _bob_qr = bob
            .begin_glance_display()
            .expect("bob also shows a QR (symmetric)");

        bob.apply_glance_scan(&alice_qr)
            .expect("bob scans alice's QR");
        let alice_id = signing_key(&alice);
        bob.handle_glance_discovery("alice-device", &alice_id);
        assert!(
            bob.ble_handshake_session_active(),
            "bob (scanner) builds an initiator session on discovering the scanned peer"
        );
        let connect: Vec<_> = bob
            .drain_pending_commands()
            .into_iter()
            .filter(|c| matches!(c, vauchi_core::Command::BleConnect { device_id } if device_id == "alice-device"))
            .collect();
        assert_eq!(
            connect.len(),
            1,
            "bob must emit exactly one BleConnect to the scanned peer"
        );

        alice.start_ble_handshake_as_responder();
        assert!(
            alice.ble_handshake_session_active(),
            "alice (displayer/peripheral) builds a responder session on connect"
        );

        run_handshake(&mut bob, &mut alice);

        assert_eq!(
            bob.vauchi.list_contacts().expect("contacts").len(),
            1,
            "scanner persists the displayer"
        );
        assert_eq!(
            alice.vauchi.list_contacts().expect("contacts").len(),
            1,
            "displayer persists the scanner"
        );
    }

    // @scenario: ble_exchange :: Glance scanner ignores an advertiser it did not scan (F1 dissolves)
    #[test]
    fn glance_orchestration_scanner_ignores_foreign_advertiser() {
        let mut alice = fresh_engine("Alice");
        let mut bob = fresh_engine("Bob");
        let alice_qr = alice.begin_glance_display().expect("alice QR");
        bob.apply_glance_scan(&alice_qr).expect("bob scans alice");

        let mallory = fresh_engine("Mallory");
        let mallory_id = signing_key(&mallory);
        bob.handle_glance_discovery("mallory-device", &mallory_id);

        assert!(
            !bob.ble_handshake_session_active(),
            "bob must not connect to an advertiser whose identity != the scanned QR"
        );
        assert!(
            bob.drain_pending_commands().is_empty(),
            "no BleConnect to a foreign advertiser (no latch race, F1 dissolves)"
        );
    }

    // @scenario: ble_exchange :: Glance identity-spoofing advertiser is rejected at the handshake pin
    #[test]
    fn glance_orchestration_identity_spoofing_advertiser_rejected_at_handshake() {
        // Mallory advertises Alice's (public) identity to satisfy bob's
        // discovery match, then answers with her own keys. The advertisement
        // match is NOT the security boundary — the session pin is.
        let mut alice = fresh_engine("Alice");
        let mut bob = fresh_engine("Bob");
        let mut mallory = fresh_engine("Mallory");

        let alice_qr = alice.begin_glance_display().expect("alice QR");
        bob.apply_glance_scan(&alice_qr).expect("bob scans alice");

        let alice_id = signing_key(&alice);
        bob.handle_glance_discovery("mallory-device", &alice_id);
        assert!(
            bob.ble_handshake_session_active(),
            "bob connects — the advertisement claimed alice's identity"
        );
        let _ = bob.drain_pending_commands();

        mallory.start_ble_handshake_as_responder();
        run_handshake(&mut bob, &mut mallory);

        assert_eq!(
            bob.vauchi.list_contacts().expect("contacts").len(),
            0,
            "the handshake pin rejects Mallory — she is not the scanned Alice"
        );
    }
}
