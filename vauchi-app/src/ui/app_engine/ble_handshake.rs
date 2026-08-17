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
                direction,
                uuid,
                data,
            } => holder
                .machine
                .on_data_received(device_id, *direction, uuid, data, now),
            Event::BleCharacteristicRead {
                device_id,
                direction,
                uuid,
                data,
            } => {
                // Frontends route a READ-response on the same UUID
                // surface as a notify; the machine's reassembler
                // handles both.
                holder
                    .machine
                    .on_data_received(device_id, *direction, uuid, data, now)
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

    /// Route one hardware event into the AppEngine-owned handshake machine,
    /// gated on an active session. Returns `true` when the machine reached a
    /// terminal event (`Completed`/`Failed`), signalling the platform layer to
    /// fire a presentation invalidation for observers that are not rendering
    /// the resulting command batch (P5b, 2026-06-10).
    ///
    /// This gate used to live in `PlatformAppEngine::handle_hardware_event`,
    /// where only the typed UniFFI seam benefited from it — an event arriving
    /// through the canonical `dispatch_json` envelope never reached the
    /// machine. It lives here so both envelopes route identically (ADR-066:
    /// one Event input). Additive on top of the regular dispatch that
    /// follows, so the `ExchangeEngine::BleExchangeFlow` proximity path runs
    /// undisturbed.
    pub fn route_ble_hardware_event_to_machine(&mut self, event: &Event) -> bool {
        if !matches!(
            event,
            Event::BleConnected { .. }
                | Event::BleCharacteristicNotified { .. }
                | Event::BleCharacteristicRead { .. }
                | Event::BleMtuNegotiated { .. }
                | Event::BleDisconnected { .. }
        ) {
            return false;
        }
        // A GATT peripheral never scans, so it emits no `BleDeviceDiscovered`
        // and the discovery routing never built its session. The peripheral
        // that gets connected to is always the responder — build that session
        // now so its KeyOffer-onward writes reach the real machine and the
        // contact persists (`2026-06-08-ios-ble-responder-persist`). No-op
        // for the central, which already holds an active session from
        // discovery.
        if matches!(event, Event::BleConnected { .. })
            && !self.ble_handshake_session_active()
            && matches!(self.current_app_screen(), AppScreen::BleExchange { .. })
        {
            self.start_ble_handshake_as_responder();
        }
        if !self.ble_handshake_session_active() {
            return false;
        }
        let m_event = self.forward_ble_hardware_event(event);
        let terminal = matches!(
            m_event,
            BleMachineEvent::Completed(_) | BleMachineEvent::Failed { .. }
        );
        self.apply_ble_machine_event(m_event);
        terminal
    }

    /// Drains the pending terminal-BLE invalidation flag. The platform layer
    /// calls this after dispatch and fires the observer invalidation when set.
    pub fn take_pending_presentation_invalidation(&mut self) -> bool {
        std::mem::take(&mut self.pending_ble_terminal_invalidation)
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
        self.exchange_was_reconnection = self.contact_already_held(contact.id());
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
// from a `tests/` integration directory. Extracted to
// ble_handshake_tests.rs to keep this file under the src size limit.
#[cfg(test)]
#[path = "ble_handshake_tests.rs"]
mod tests;
