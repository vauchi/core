// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE handshake state machine (slice 32m T2.2a).
//!
//! Lifts the encrypted four-phase BLE exchange driver out of
//! `vauchi-platform/src/mobile_ble.rs` (1007 LOC) into a
//! deterministic, synchronous machine the engine owns and feeds
//! through the `Event` / `Command` protocol (ADR-031). Replaces the
//! `MobileBleDelegate` callback trait and `MobileBleExchangeSession`
//! UniFFI object with a pure input/output state machine — the same
//! pattern the slice 32m T1.2 series applied to multi-stage exchange.
//!
//! Pattern source: slice 32l's
//! [`DeviceLinkInitiatorMachine`](super::device_link_machine::DeviceLinkInitiatorMachine)
//! and slice 32m T1.2b's
//! [`MultiStageMachine`](super::multi_stage_machine::MultiStageMachine).
//! Same five-method shape (`new` / `state` / `handle_hardware_event`
//! / `cancel` / a side-effect-emitting drive method), same `now: u64`
//! time discipline (no `Sleeper`, no `Clock`, no thread, no mpsc —
//! CC-06).
//!
//! Design: `_private/docs/designs/2026-05-28-slice-32m-phase-0-event-command-mapping-design.md` §3.
//!
//! # Protocol overview (mirrors the cycle-thread `mobile_ble.rs`)
//!
//! Four-phase encrypted exchange:
//!
//! - **Phase 1** — Initiator: `create_key_offer` → write
//!   `CHAR_HANDSHAKE_WRITE` with the offer. Responder: receive on
//!   `CHAR_HANDSHAKE_WRITE`, `process_key_offer` → write
//!   `CHAR_HANDSHAKE_NOTIFY` with the KeyAck + send our encrypted
//!   card as chunks on `CHAR_DATA_NOTIFY`.
//! - **Phase 2** — Initiator: receive on `CHAR_HANDSHAKE_NOTIFY`
//!   (KeyAck) + chunked encrypted card on `CHAR_DATA_NOTIFY`,
//!   reassemble, `process_key_ack` → write commitment on
//!   `CHAR_HANDSHAKE_WRITE` + our encrypted card chunks on
//!   `CHAR_DATA_WRITE`.
//! - **Phase 3** — Responder: receive commitment + chunked encrypted
//!   card on `CHAR_HANDSHAKE_WRITE` / `CHAR_DATA_WRITE`, reassemble,
//!   `process_committed_payload` → write reveal on
//!   `CHAR_HANDSHAKE_NOTIFY`.
//! - **Phase 4** — Initiator: receive reveal on
//!   `CHAR_HANDSHAKE_NOTIFY`, `complete_exchange`. Responder:
//!   already in `PayloadsExchanged`; same path.
//!
//! Subscribe-notify Commands are NOT emitted — per T0.2 design §3.1
//! the favored hypothesis (verified by T2.1's
//! `happy_path_emits_no_subscribe_notify_command` test) is that
//! frontends auto-subscribe on connect via Manifest +
//! `writeDescriptor(ENABLE_NOTIFICATION_VALUE)`. T3.1 retires the
//! `subscribe_notify` delegate method outright.

use vauchi_core::Command;
use vauchi_core::crypto::X3DHKeyPair;
use vauchi_core::exchange::{
    BLE_CHUNK_OVERHEAD, BLE_DEFAULT_USABLE, BleCardPayload, BleChunker, BleExchangeResult,
    BleHandshakeSession, BleHandshakeState, BleReassembler, CHAR_DATA_NOTIFY, CHAR_DATA_WRITE,
    CHAR_HANDSHAKE_NOTIFY, CHAR_HANDSHAKE_WRITE, KEY_ACK_SIZE,
};

/// Role on the BLE exchange — initiator sends the KeyOffer first,
/// responder waits for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BleRole {
    Initiator,
    Responder,
}

/// Decide this device's BLE role from the symmetric role-tiebreak
/// tokens. Both peers advertise + scan symmetrically; the device whose
/// token is lexicographically smaller becomes the **initiator**
/// (central, connects + sends the KeyOffer), the other the
/// **responder** (peripheral). Equal tokens — effectively impossible
/// for distinct identities — default to responder so neither side
/// double-connects.
///
/// This is the single source of truth for the tiebreak, shared by the
/// chrome-side [`super::super::ui::exchange::ble::BleExchangeFlow`]
/// (decides whether to emit `BleConnect`) and the crypto-side
/// `AppEngine::start_ble_handshake_on_discovery` (decides the session
/// role). Keeping it in one place means the two can never disagree.
pub fn decide_ble_role(own_token: &[u8], peer_token: &[u8]) -> BleRole {
    if own_token < peer_token {
        BleRole::Initiator
    } else {
        BleRole::Responder
    }
}

/// Observable phase of the BLE handshake machine. 1:1 with
/// [`BleHandshakeState`] (renamed for engine-side ergonomics — the
/// underlying protocol model is unchanged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BleMachinePhase {
    /// Constructed, pre-connection. No commands emitted yet.
    Preparing,
    /// Driving the handshake — KeyOffer / KeyAck / commitment exchange
    /// in flight.
    Handshaking,
    /// Bulk encrypted-card chunk transfer in flight.
    Transferring,
    /// Both payloads exchanged; awaiting / sending reveal.
    Verifying,
    /// Exchange completed — peer card available.
    Completed,
    /// Terminal failure.
    Failed { reason: String },
    /// User-initiated cancel (absorbing).
    Cancelled,
}

/// What a transition produced. Engine integration maps each onto the
/// existing `BleExchangeFlow` setters / `ActionResult`s.
#[derive(Debug, Clone)]
pub enum BleMachineEvent {
    /// No observable change this step.
    None,
    /// Transferring entered (KeyAck observed).
    TransferringStarted,
    /// Verifying entered (both payloads exchanged).
    VerifyingStarted,
    /// Exchange completed; the peer's card + ratchet keys are
    /// available in the carried result.
    Completed(Box<BleExchangeResult>),
    /// The peer's reciprocity confirmation ack verified (P1 step 2b): the
    /// exchange is now mutually confirmed. Carries the peer's identity key so
    /// the AppEngine can flip that contact's reciprocity to Confirmed. NOT a
    /// terminal event — the exchange already completed and persisted.
    ReciprocityConfirmed { their_identity: [u8; 32] },
    /// Terminal failure.
    Failed { reason: String },
}

/// Out-of-band material threaded into a [`BleHandshakeMachine`] when the
/// exchange was bootstrapped over an OOB channel (scanned QR / NFC tap).
///
/// Each field is independently optional; a `None` field leaves that check
/// inactive, and `None` for the whole binding is the radio-only case
/// (Magic/Bump/Shake have no OOB peer). The *caller* populates the fields for
/// its role — the connector/scanner pins [`Self::expected_peer`] and echoes
/// [`Self::oob_nonce_echo`]; the advertiser/displayer sets
/// [`Self::required_oob_nonce`]. The machine is a neutral conduit: it forwards
/// present fields to the session and makes no role decision itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct BleOobBinding {
    /// Pin the expected wire identity; a mismatch aborts (scanner side).
    pub expected_peer: Option<[u8; 32]>,
    /// Pin the expected wire X25519 exchange key (the DH input). Without this,
    /// the identity pin is cosmetic — the Ed25519 identity never enters the DH
    /// (scanner side).
    pub expected_exchange_key: Option<[u8; 32]>,
    /// Carry this OOB nonce in our KeyOffer so the displayer can verify we
    /// saw the QR/tap (scanner side).
    pub oob_nonce_echo: Option<[u8; 16]>,
    /// Require the peer's KeyOffer to echo this nonce; absence/mismatch
    /// aborts (displayer side).
    pub required_oob_nonce: Option<[u8; 16]>,
}

/// Forward the present [`BleOobBinding`] fields to the session before the
/// handshake advances. No-op for the radio-only (`None`) case.
fn apply_oob(inner: &mut BleHandshakeSession, oob: Option<BleOobBinding>) {
    let Some(binding) = oob else { return };
    if let Some(peer) = binding.expected_peer {
        inner.expect_peer(peer);
    }
    if let Some(exchange) = binding.expected_exchange_key {
        inner.expect_exchange_key(exchange);
    }
    if let Some(nonce) = binding.oob_nonce_echo {
        inner.set_oob_nonce(nonce);
    }
    if let Some(nonce) = binding.required_oob_nonce {
        inner.require_oob_nonce(nonce);
    }
}

/// Deterministic, event-driven BLE handshake machine.
///
/// Owns the inner [`BleHandshakeSession`] plus the chunker /
/// reassembler / mtu_usable state that lived in
/// `MobileBleExchangeSession` pre-32m. Replaces both the
/// `MobileBleDelegate` callback trait (frontends now consume
/// `Command::BleWriteCharacteristic` / `Command::BleDisconnect`)
/// and the `MobileBleExchangeSession` UniFFI object (`AppEngine`
/// holds the machine; the binding crate's surface narrows to
/// dispatching `Event`s through `handle_hardware_event`).
pub struct BleHandshakeMachine {
    inner: BleHandshakeSession,
    role: BleRole,
    reassembler: Option<BleReassembler>,
    /// Cached intermediate payload between protocol steps:
    /// - Initiator post-KeyAck: holds the KeyAck bytes while we
    ///   reassemble the responder's encrypted card.
    /// - Responder post-commitment: holds the commitment bytes while
    ///   we reassemble the initiator's encrypted card.
    pending_intermediate: Option<Vec<u8>>,
    /// Usable payload bytes per BLE chunk. Updated by
    /// [`Self::update_mtu`]; defaults to [`BLE_DEFAULT_USABLE`] when
    /// no negotiation has happened.
    mtu_usable: usize,
    /// Phase derived from `inner.state()`, overridden by terminal
    /// transitions (`Cancelled`, host-side `Failed { reason }`).
    phase: BleMachinePhase,
    /// `true` once `cancel` has been called. Subsequent ingress
    /// returns [`BleMachineEvent::None`] and the phase stays
    /// [`BleMachinePhase::Cancelled`].
    cancelled: bool,
}

impl BleHandshakeMachine {
    /// Construct an initiator machine. No I/O — the KeyOffer is
    /// created on the first `on_connected` call.
    ///
    /// `oob` supplies bootstrapped-mode pin/nonce material (Glance QR / NFC);
    /// `None` is the radio-only case.
    pub fn new_initiator(
        identity_key: [u8; 32],
        identity_x3dh: X3DHKeyPair,
        card: BleCardPayload,
        now: u64,
        oob: Option<BleOobBinding>,
    ) -> Self {
        let mut inner =
            BleHandshakeSession::new_initiator_from_key(identity_key, identity_x3dh, card, now);
        apply_oob(&mut inner, oob);
        Self {
            inner,
            role: BleRole::Initiator,
            reassembler: None,
            pending_intermediate: None,
            mtu_usable: BLE_DEFAULT_USABLE,
            phase: BleMachinePhase::Preparing,
            cancelled: false,
        }
    }

    /// Construct a responder machine. No I/O — the KeyOffer ingress
    /// happens on the first `CHAR_HANDSHAKE_WRITE` notification.
    ///
    /// `oob` supplies bootstrapped-mode pin/nonce material (Glance QR / NFC);
    /// `None` is the radio-only case.
    pub fn new_responder(
        identity_key: [u8; 32],
        identity_x3dh: X3DHKeyPair,
        card: BleCardPayload,
        now: u64,
        oob: Option<BleOobBinding>,
    ) -> Self {
        let mut inner =
            BleHandshakeSession::new_responder_from_key(identity_key, identity_x3dh, card, now);
        apply_oob(&mut inner, oob);
        Self {
            inner,
            role: BleRole::Responder,
            reassembler: None,
            pending_intermediate: None,
            mtu_usable: BLE_DEFAULT_USABLE,
            phase: BleMachinePhase::Preparing,
            cancelled: false,
        }
    }

    /// Current observable phase.
    pub fn phase(&self) -> BleMachinePhase {
        self.phase.clone()
    }

    /// Inner protocol state — test seam and engine-integration
    /// convenience for callers that need a finer view than `phase()`.
    pub fn handshake_state(&self) -> &BleHandshakeState {
        self.inner.state()
    }

    /// Role marker — read-only.
    pub fn role(&self) -> BleRole {
        self.role
    }

    /// The handshake's derived session key, once key agreement has
    /// produced it. Used to persist the exchanged contact (transport
    /// key + Double Ratchet seed) when the machine reaches `Completed`.
    pub fn session_key(&self) -> Option<&vauchi_core::crypto::SymmetricKey> {
        self.inner.session_key()
    }

    /// Build the post-persist reciprocity confirmation ack (design P1) as a BLE
    /// write on the handshake-notify characteristic. The AppEngine calls this
    /// ONLY after durably persisting the contact (G1 ordering: "peer received
    /// my token ⇒ I persisted") and queues the command; the peer verifies it
    /// against its `expected_their_token`. `None` until key agreement derived
    /// the tokens.
    pub fn build_reciprocity_ack_command(&self) -> Option<Command> {
        let ack = self.inner.build_reciprocity_ack()?;
        Some(Command::BleWriteCharacteristic {
            uuid: CHAR_HANDSHAKE_NOTIFY.to_string(),
            data: ack,
        })
    }

    /// Currently negotiated usable MTU (payload bytes per chunk).
    pub fn mtu_usable(&self) -> usize {
        self.mtu_usable
    }

    /// Whether this machine is in a terminal phase. `true` for
    /// `Completed`, `Failed`, `Cancelled`; `false` otherwise.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.phase,
            BleMachinePhase::Completed
                | BleMachinePhase::Failed { .. }
                | BleMachinePhase::Cancelled
        )
    }

    /// Update the chunker's MTU window. Called on
    /// `Event::BleMtuNegotiated`. Idempotent under re-negotiation:
    /// a second call mid-session overwrites cleanly.
    pub fn update_mtu(&mut self, mtu: u32) {
        let usable = (mtu as usize).saturating_sub(3);
        self.mtu_usable = usable.max(BLE_CHUNK_OVERHEAD + 1);
    }

    /// Called when the BLE connection completes. For initiators
    /// this kicks the handshake by creating + sending the KeyOffer.
    /// For responders this is a no-op (the protocol waits for the
    /// initiator's KeyOffer on `CHAR_HANDSHAKE_WRITE`).
    pub fn on_connected(&mut self, _now: u64) -> (BleMachineEvent, Vec<Command>) {
        if self.cancelled || self.is_terminal() {
            return (BleMachineEvent::None, Vec::new());
        }
        if self.role == BleRole::Responder {
            // Frontends auto-subscribe on connect (T0.2 §3.1
            // hypothesis); no subscribe Command emitted.
            self.phase = BleMachinePhase::Handshaking;
            return (BleMachineEvent::None, Vec::new());
        }
        // Idempotency: a duplicate `BleConnected` can arrive because the
        // initiator is ALSO a peripheral — its own GATT server accepts the
        // peer's connection, producing a second connect event. The KeyOffer is
        // created exactly once; `create_key_offer` requires the `Idle` inner
        // state, so a second call hit `InvalidState` and `mark_failed`,
        // killing the handshake just as the peer's KeyAck arrived. If the offer
        // already exists (state advanced past `Idle`), the duplicate is a no-op.
        if !matches!(self.inner.state(), BleHandshakeState::Idle) {
            return (BleMachineEvent::None, Vec::new());
        }
        match self.inner.create_key_offer() {
            Ok(offer) => {
                self.phase = BleMachinePhase::Handshaking;
                let cmd = Command::BleWriteCharacteristic {
                    uuid: CHAR_HANDSHAKE_WRITE.into(),
                    data: offer,
                };
                (BleMachineEvent::None, vec![cmd])
            }
            Err(e) => self.mark_failed(format!("Failed to create key offer: {e:?}")),
        }
    }

    /// Called when a BLE characteristic notification arrives. The
    /// `uuid` selects the handler; the `data` is fed through the
    /// reassembler for chunked characteristics (data write / data
    /// notify) and consumed directly for handshake characteristics.
    pub fn on_data_received(
        &mut self,
        uuid: &str,
        data: &[u8],
        now: u64,
    ) -> (BleMachineEvent, Vec<Command>) {
        // P1 step 2b: once we've Completed, a notify on the handshake channel
        // may be the peer's post-persist reciprocity ack. Handle it BEFORE the
        // terminal guard drops it. `process_reciprocity_ack` rejects anything
        // that is not a valid ack (wrong version / undecryptable / token
        // mismatch), so residue never yields a false Confirmed.
        if matches!(self.phase, BleMachinePhase::Completed) && uuid == CHAR_HANDSHAKE_NOTIFY {
            if let Ok(Some(their_identity)) = self.inner.process_reciprocity_ack(data) {
                return (
                    BleMachineEvent::ReciprocityConfirmed { their_identity },
                    Vec::new(),
                );
            }
            return (BleMachineEvent::None, Vec::new());
        }
        if self.cancelled || self.is_terminal() {
            return (BleMachineEvent::None, Vec::new());
        }
        match uuid {
            CHAR_HANDSHAKE_WRITE => self.handle_handshake_write(data, now),
            CHAR_HANDSHAKE_NOTIFY => self.handle_handshake_notify(data),
            CHAR_DATA_WRITE | CHAR_DATA_NOTIFY => self.handle_data_chunk(data, now),
            _ => (BleMachineEvent::None, Vec::new()),
        }
    }

    /// Called when the BLE peer disconnects.
    pub fn on_disconnected(&mut self, reason: &str) -> (BleMachineEvent, Vec<Command>) {
        if self.cancelled || self.is_terminal() {
            return (BleMachineEvent::None, Vec::new());
        }
        self.mark_failed(format!("BLE disconnected: {reason}"))
    }

    /// User-initiated cancel. Idempotent.
    pub fn cancel(&mut self) -> Vec<Command> {
        if self.cancelled {
            return Vec::new();
        }
        self.cancelled = true;
        self.phase = BleMachinePhase::Cancelled;
        vec![Command::BleDisconnect]
    }

    // ── Internal handlers ──────────────────────────────────────────

    fn handle_handshake_write(&mut self, data: &[u8], now: u64) -> (BleMachineEvent, Vec<Command>) {
        // Initiators never receive on the handshake-write
        // characteristic in the documented protocol; ignore.
        if self.role == BleRole::Initiator {
            return (BleMachineEvent::None, Vec::new());
        }
        match self.inner.state() {
            BleHandshakeState::Idle => {
                // Phase 1 responder ingress: KeyOffer.
                match self.inner.process_key_offer(data, now) {
                    Ok((ack_bytes, encrypted_card)) => {
                        let mut cmds =
                            Vec::with_capacity(2 + chunk_count(&encrypted_card, self.mtu_usable));
                        // KeyAck on handshake notify.
                        cmds.push(Command::BleWriteCharacteristic {
                            uuid: CHAR_HANDSHAKE_NOTIFY.into(),
                            data: ack_bytes,
                        });
                        // Our encrypted card chunks on data notify.
                        cmds.extend(self.chunk_to_commands(&encrypted_card, CHAR_DATA_NOTIFY));
                        self.phase = BleMachinePhase::Transferring;
                        (BleMachineEvent::TransferringStarted, cmds)
                    }
                    Err(e) => self.mark_failed(format!("Failed to process key offer: {e:?}")),
                }
            }
            _ => {
                // Phase 3 responder ingress: commitment (first 32 B)
                // followed by chunked encrypted card on data write.
                // Stash the commitment; the chunks arrive separately.
                if data.len() >= 32 {
                    self.pending_intermediate = Some(data[..32].to_vec());
                }
                (BleMachineEvent::None, Vec::new())
            }
        }
    }

    fn handle_handshake_notify(&mut self, data: &[u8]) -> (BleMachineEvent, Vec<Command>) {
        match (self.role, self.inner.state()) {
            (BleRole::Initiator, BleHandshakeState::KeyOfferSent { .. }) => {
                // Phase 2 initiator ingress: KeyAck. A wire KeyAck is exactly
                // KEY_ACK_SIZE bytes — anything else is quarantine-dropped
                // BEFORE buffering so pending_intermediate never holds an
                // attacker-sized allocation (DC-01,
                // backlog/2026-07-20-ble-exchange-orchestrator-unification).
                // Dropped, not failed: the genuine KeyAck may still arrive.
                if data.len() != KEY_ACK_SIZE {
                    return (BleMachineEvent::None, Vec::new());
                }
                // Stash for process_key_ack once chunks complete; flip to
                // Transferring chrome.
                self.pending_intermediate = Some(data.to_vec());
                self.phase = BleMachinePhase::Transferring;
                (BleMachineEvent::TransferringStarted, Vec::new())
            }
            (BleRole::Initiator, BleHandshakeState::PayloadsExchanged { .. })
            | (BleRole::Responder, BleHandshakeState::PayloadsExchanged { .. }) => {
                // Phase 4 reveal — both roles complete here.
                self.complete_with_reveal(data)
            }
            _ => (BleMachineEvent::None, Vec::new()),
        }
    }

    fn handle_data_chunk(&mut self, data: &[u8], now: u64) -> (BleMachineEvent, Vec<Command>) {
        if data.len() < BLE_CHUNK_OVERHEAD {
            return (BleMachineEvent::None, Vec::new());
        }
        let total = u16::from_le_bytes([data[2], data[3]]);
        if self.reassembler.is_none() {
            match BleReassembler::new(total) {
                Ok(r) => self.reassembler = Some(r),
                Err(e) => return self.mark_failed(format!("Reassembler creation failed: {e:?}")),
            }
        }
        let reass = self
            .reassembler
            .as_mut()
            .expect("reassembler set above on first chunk");
        if let Err(e) = reass.insert_chunk(data) {
            return self.mark_failed(format!("Chunk reassembly failed: {e:?}"));
        }
        if !reass.is_complete() {
            return (BleMachineEvent::None, Vec::new());
        }
        let assembled = reass
            .assemble()
            .expect("reassembler reports complete; assemble cannot fail");
        // Reset reassembler for any subsequent chunked transfer
        // (initiator post-KeyAck sends its OWN encrypted card on
        // data write; we may reassemble peer's reveal-time payload
        // in a future protocol extension).
        self.reassembler = None;
        self.on_remote_encrypted_card_received(&assembled, now)
    }

    fn on_remote_encrypted_card_received(
        &mut self,
        encrypted_card: &[u8],
        now: u64,
    ) -> (BleMachineEvent, Vec<Command>) {
        match self.role {
            BleRole::Initiator => {
                let Some(ack_bytes) = self.pending_intermediate.take() else {
                    return self.mark_failed("No pending KeyAck data".into());
                };
                match self.inner.process_key_ack(&ack_bytes, encrypted_card, now) {
                    Ok((commitment, our_encrypted)) => {
                        let mut cmds =
                            Vec::with_capacity(1 + chunk_count(&our_encrypted, self.mtu_usable));
                        cmds.push(Command::BleWriteCharacteristic {
                            uuid: CHAR_HANDSHAKE_WRITE.into(),
                            data: commitment,
                        });
                        cmds.extend(self.chunk_to_commands(&our_encrypted, CHAR_DATA_WRITE));
                        self.phase = BleMachinePhase::Verifying;
                        (BleMachineEvent::VerifyingStarted, cmds)
                    }
                    Err(e) => self.mark_failed(format!("Failed to process key ack: {e:?}")),
                }
            }
            BleRole::Responder => {
                let Some(commitment) = self.pending_intermediate.take() else {
                    return self.mark_failed("No pending commitment".into());
                };
                let reveal = match self
                    .inner
                    .process_committed_payload(&commitment, encrypted_card)
                {
                    Ok(reveal) => reveal,
                    Err(e) => {
                        return self
                            .mark_failed(format!("Failed to process committed payload: {e:?}"));
                    }
                };
                // Send our reveal so the initiator can finalize, then
                // complete our own side immediately. By Phase 3 the
                // responder has already verified the initiator's
                // commitment and holds the (decryptable) reciprocal card,
                // so `complete_exchange` takes an empty reveal (per its
                // doc: "responder: reveal is empty, already verified in
                // Phase 3"). Without this the responder sat in `Verifying`
                // forever — the initiator emits nothing after it
                // completes, so no closing notification ever arrived and
                // the responder never persisted the contact.
                let reveal_cmd = Command::BleWriteCharacteristic {
                    uuid: CHAR_HANDSHAKE_NOTIFY.into(),
                    data: reveal,
                };
                match self.inner.complete_exchange(&[]) {
                    Ok(result) => {
                        self.phase = BleMachinePhase::Completed;
                        (
                            BleMachineEvent::Completed(Box::new(result)),
                            vec![reveal_cmd],
                        )
                    }
                    Err(e) => self.mark_failed(format!("Responder completion failed: {e:?}")),
                }
            }
        }
    }

    fn complete_with_reveal(&mut self, reveal: &[u8]) -> (BleMachineEvent, Vec<Command>) {
        match self.inner.complete_exchange(reveal) {
            Ok(result) => {
                self.phase = BleMachinePhase::Completed;
                (BleMachineEvent::Completed(Box::new(result)), Vec::new())
            }
            Err(e) => self.mark_failed(format!("Exchange verification failed: {e:?}")),
        }
    }

    fn mark_failed(&mut self, reason: String) -> (BleMachineEvent, Vec<Command>) {
        tracing::warn!("[Exchange] BLE handshake failed: {reason}");
        self.phase = BleMachinePhase::Failed {
            reason: reason.clone(),
        };
        (BleMachineEvent::Failed { reason }, Vec::new())
    }

    fn chunk_to_commands(&self, data: &[u8], uuid: &str) -> Vec<Command> {
        let chunker = BleChunker::new(data, self.mtu_usable);
        (0..chunker.total_chunks())
            .filter_map(|i| {
                chunker
                    .chunk(i)
                    .map(|payload| Command::BleWriteCharacteristic {
                        uuid: uuid.into(),
                        data: payload,
                    })
            })
            .collect()
    }
}

/// Predict chunk count without materialising the chunker — used
/// only for `Vec::with_capacity` hints. Off-by-one is harmless.
fn chunk_count(data: &[u8], mtu_usable: usize) -> usize {
    let payload_size = mtu_usable.saturating_sub(BLE_CHUNK_OVERHEAD).max(1);
    if data.is_empty() {
        1
    } else {
        data.len().div_ceil(payload_size)
    }
}
