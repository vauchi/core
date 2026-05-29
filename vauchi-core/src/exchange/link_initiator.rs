// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure-Rust state machine driving the initiator side of link-mode
//! contact exchange.
//!
//! Mirror of [`crate::exchange::link_responder::LinkResponderSession`]
//! (slice 32l Phase 2) for the initiator half. Inputs are
//! [`LinkInitiatorSession::apply_hardware_event`] (relay events arriving
//! through the engine's hardware-event path) and
//! [`LinkInitiatorSession::tick`] (wall-clock checks for the polling
//! deadline). Outputs are
//! [`LinkInitiatorSession::drain_pending_commands`] (commands the engine
//! dispatches to the relay surface) and
//! [`LinkInitiatorSession::current_state`].
//!
//! Unlike the responder — which polls a single escrow gate — the
//! initiator runs a two-gate flow:
//!
//! 1. **Handshake gate.** On [`crate::platform::Event::LinkShared`] the
//!    session asks the relay to watch the handshake gate. When the
//!    responder deposits its ephemeral public key the gate crosses two
//!    slots and [`crate::platform::Event::RelayEscrowReady`] fires for
//!    the handshake gate; the session retrieves the responder's epk.
//! 2. **Escrow gate.** On [`crate::platform::Event::LinkOpened`] (the
//!    retrieved epk) the session performs ECDH, derives
//!    [`EscrowKeys`], encrypts the initiator's card, deposits it, and
//!    starts polling the escrow gate. `RelayEscrowReady` for the escrow
//!    gate triggers retrieval of the responder's card, and
//!    [`crate::platform::Event::RelayEscrowBlobReceived`] decrypts it to
//!    `Finalized`.
//!
//! The engine (in `core/vauchi-app/src/ui/app_engine/link_exchange.rs`)
//! wraps this state machine, dispatches the commands via the frontend's
//! relay surface, and forwards events back. Keeping the state machine
//! pure Rust (no threads, no sleeps) lets it be exercised
//! deterministically by tests and a proptest for the card round-trip.
//!
//! Real crypto throughout (ADR-002 — no mocks).
//!
//! See `_private/docs/problems/2026-05-11-link-exchange-engine-graduation/`.

use crate::exchange::escrow::EscrowKeys;
use crate::exchange::link_mode::{self, LinkInitiation, LinkModeError};
use crate::platform::{Command, Event};

/// Polling interval (ms) the initiator suggests to the relay for the
/// handshake gate. Mirrors `link.rs::LINK_POLL_INTERVAL_MS` — both
/// sides poll on the same 30 s cadence, with relay-side backoff capping
/// at 5 min.
const HANDSHAKE_POLL_INTERVAL_MS: u32 = 30_000;

/// Once the escrow card has been deposited the user is actively waiting,
/// so the escrow gate is polled aggressively (matches `link.rs`).
const ESCROW_POLL_INTERVAL_MS: u32 = 1_000;

/// Why the initiator cycle ended without a finalized contact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkInitiatorFailureReason {
    /// Polling exhausted the deadline without completing the exchange.
    PollingTimedOut,
    /// The responder's ephemeral public key was malformed (wrong length)
    /// or produced a non-contributory Diffie-Hellman output.
    HandshakeFailed { detail: String },
    /// Card retrieved from escrow but symmetric decryption failed.
    DecryptError { detail: String },
    /// `RelayEscrowFailed` arrived for one of our gates.
    DepositRejected,
    /// User-initiated cancel via the waiting/share screen's Cancel
    /// action (or the engine's lifecycle teardown on navigate-back).
    Cancelled,
}

/// State of a link-mode initiator cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkInitiatorState {
    /// URL shared (or pending share). Waiting for the responder to open
    /// the link, deposit its epk, and for the handshake gate to be
    /// retrieved + ECDH to complete.
    Polling,
    /// ECDH complete, our card deposited; waiting for the responder's
    /// encrypted card on the escrow gate, then retrieving + decrypting.
    Retrieving,
    /// Responder's card retrieved + decrypted; carries the plaintext
    /// card-payload bytes the engine hands to its persistence layer.
    Finalized { card_bytes: Vec<u8> },
    /// Terminal failure with typed reason.
    Failed(LinkInitiatorFailureReason),
}

/// Pure-Rust state machine for the initiator side of link mode.
pub struct LinkInitiatorSession {
    state: LinkInitiatorState,
    /// Initiation data (URL, ephemeral secret, handshake/presence slots).
    initiation: LinkInitiation,
    /// The initiator's own framed card payload (`serialize_card_payload`
    /// output) to encrypt + deposit once ECDH completes.
    own_card_bytes: Vec<u8>,
    /// Escrow keys derived after ECDH with the responder. `None` until
    /// [`Event::LinkOpened`] is processed.
    escrow_keys: Option<EscrowKeys>,
    /// Decoded handshake-gate bytes, cached so per-event comparisons are
    /// O(32) byte equality.
    handshake_gate_bytes: Vec<u8>,
    /// Unix-seconds deadline for the whole cycle. Once reached without a
    /// terminal state, `tick` transitions to `Failed(PollingTimedOut)`.
    poll_deadline_unix: u64,
    /// Commands queued for `drain_pending_commands`. One-shot per call.
    pending: Vec<Command>,
}

impl LinkInitiatorSession {
    /// Construct a new initiator session.
    ///
    /// `initiation` and `presence_commands` come from
    /// [`link_mode::initiator_generate`]; the presence deposit must
    /// reach the relay before the URL is shared, so the session queues
    /// `presence_commands` as its initial drain. `own_card_bytes` is the
    /// framed payload ([`link_mode::serialize_card_payload`]) the
    /// initiator will encrypt and deposit once ECDH completes.
    ///
    /// `poll_deadline_unix` is the unix-seconds time after which the
    /// session transitions to `Failed(PollingTimedOut)` if it has not
    /// reached a terminal state.
    pub fn new(
        initiation: LinkInitiation,
        presence_commands: Vec<Command>,
        own_card_bytes: Vec<u8>,
        poll_deadline_unix: u64,
    ) -> Self {
        let handshake_gate_bytes =
            hex::decode(&initiation.handshake_slot).expect("hex from hex::encode is always valid");

        Self {
            state: LinkInitiatorState::Polling,
            initiation,
            own_card_bytes,
            escrow_keys: None,
            handshake_gate_bytes,
            poll_deadline_unix,
            pending: presence_commands,
        }
    }

    /// Current state — read-only accessor for the engine + tests.
    pub fn current_state(&self) -> &LinkInitiatorState {
        &self.state
    }

    /// The URL to share. Surfaced so the engine can render the
    /// share-url screen and emit the share-sheet command.
    pub fn share_url(&self) -> &str {
        &self.initiation.url
    }

    /// Drain queued commands. Idempotent: a second call returns empty.
    pub fn drain_pending_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.pending)
    }

    /// Apply a hardware event from the relay. Events for unrelated gates
    /// are ignored. Terminal states (`Finalized` / `Failed`) ignore all
    /// events — once a session has completed, further events do not
    /// unset its outcome.
    pub fn apply_hardware_event(&mut self, event: Event) {
        if self.is_terminal() {
            return;
        }

        match event {
            // The frontend reports the URL was shared — start watching
            // the handshake gate for the responder's epk deposit.
            Event::LinkShared => {
                if matches!(self.state, LinkInitiatorState::Polling) {
                    self.pending.push(Command::RelayEscrowCheck {
                        gate_hash: self.handshake_gate_bytes.clone(),
                        suggested_interval_ms: HANDSHAKE_POLL_INTERVAL_MS,
                    });
                }
            }
            Event::RelayEscrowReady { gate_hash } => {
                if gate_hash == self.handshake_gate_bytes {
                    // Handshake gate crossed two slots — retrieve the
                    // responder's epk from our presence slot.
                    if matches!(self.state, LinkInitiatorState::Polling) {
                        let slot = hex::decode(&self.initiation.presence_slot)
                            .expect("hex from hex::encode is always valid");
                        self.pending.push(Command::RelayEscrowRetrieve {
                            gate_hash: self.handshake_gate_bytes.clone(),
                            slot_hash: slot,
                        });
                    }
                } else if let Some(keys) = &self.escrow_keys {
                    // Escrow gate ready — retrieve the responder's card.
                    let escrow_gate =
                        hex::decode(&keys.gate_hash).expect("hex from hex::encode is always valid");
                    if gate_hash == escrow_gate
                        && matches!(self.state, LinkInitiatorState::Retrieving)
                    {
                        let slot = hex::decode(&keys.our_slot)
                            .expect("hex from hex::encode is always valid");
                        self.pending.push(Command::RelayEscrowRetrieve {
                            gate_hash: escrow_gate,
                            slot_hash: slot,
                        });
                    }
                }
            }
            // The responder's epk was retrieved — derive keys, encrypt
            // our card, deposit it, and start polling the escrow gate.
            Event::LinkOpened { peer_public_key } => {
                if matches!(self.state, LinkInitiatorState::Polling) {
                    self.handle_link_opened(&peer_public_key);
                }
            }
            Event::RelayEscrowBlobReceived { gate_hash, blob } => {
                let Some(keys) = &self.escrow_keys else {
                    return;
                };
                let escrow_gate =
                    hex::decode(&keys.gate_hash).expect("hex from hex::encode is always valid");
                if gate_hash != escrow_gate {
                    return;
                }
                if !matches!(self.state, LinkInitiatorState::Retrieving) {
                    return;
                }
                match keys.decrypt_card(&blob) {
                    Ok(card_bytes) => {
                        self.state = LinkInitiatorState::Finalized { card_bytes };
                    }
                    Err(e) => {
                        self.fail(LinkInitiatorFailureReason::DecryptError {
                            detail: e.to_string(),
                        });
                    }
                }
            }
            Event::RelayEscrowFailed { .. } => {
                self.fail(LinkInitiatorFailureReason::DepositRejected);
            }
            _ => {
                // Other hardware events (BLE / NFC / camera / direct
                // transport / image picking) are not relevant to the
                // link-mode initiator cycle.
            }
        }
    }

    /// Unix-seconds deadline check. Called from the engine's poll tick;
    /// tests pass arbitrary `now_unix` values.
    pub fn tick(&mut self, now_unix: u64) {
        if self.is_terminal() {
            return;
        }
        if now_unix >= self.poll_deadline_unix {
            self.fail(LinkInitiatorFailureReason::PollingTimedOut);
        }
    }

    /// User-initiated cancel. No-op if the session has already reached a
    /// terminal state.
    pub fn cancel(&mut self) {
        if self.is_terminal() {
            return;
        }
        self.fail(LinkInitiatorFailureReason::Cancelled);
    }

    /// Decoded handshake-gate bytes — exposed for tests so they can
    /// build `RelayEscrowReady` events without re-decoding the hex
    /// string.
    pub fn handshake_gate_bytes(&self) -> Vec<u8> {
        self.handshake_gate_bytes.clone()
    }

    /// Decoded escrow-gate bytes once ECDH has completed — exposed for
    /// tests building escrow-phase events. `None` before `LinkOpened`.
    pub fn escrow_gate_bytes(&self) -> Option<Vec<u8>> {
        self.escrow_keys
            .as_ref()
            .map(|keys| hex::decode(&keys.gate_hash).expect("hex from hex::encode is always valid"))
    }

    /// Test-only helper: encrypt bytes with the responder's escrow key so
    /// tests can construct a round-trip blob the initiator decrypts.
    /// Returns `None` before ECDH has derived `escrow_keys`.
    ///
    /// Marked `#[cfg(any(test, feature = "testing"))]` so it is not part
    /// of the production surface.
    #[cfg(any(test, feature = "testing"))]
    pub fn test_encrypt_card(
        &self,
        plaintext: &[u8],
    ) -> Option<Result<Vec<u8>, crate::crypto::encryption::EncryptionError>> {
        self.escrow_keys
            .as_ref()
            .map(|keys| keys.encrypt_card(plaintext))
    }

    fn handle_link_opened(&mut self, peer_public_key: &[u8]) {
        let result = (|| -> Result<(EscrowKeys, Vec<Command>), LinkModeError> {
            let epk: [u8; 32] =
                peer_public_key
                    .try_into()
                    .map_err(|_| LinkModeError::MalformedPeerKey {
                        received: peer_public_key.len(),
                    })?;
            let keys = link_mode::initiator_derive_keys(&self.initiation.secret_key_bytes, &epk)?;
            let encrypted = keys
                .encrypt_card(&self.own_card_bytes)
                .map_err(|e| LinkModeError::CardCryptoFailed(e.to_string()))?;
            let commands = link_mode::build_initiator_deposit(&keys, encrypted);
            Ok((keys, commands))
        })();

        match result {
            Ok((keys, mut commands)) => {
                let escrow_gate =
                    hex::decode(&keys.gate_hash).expect("hex from hex::encode is always valid");
                // Start polling the escrow gate (responder already
                // deposited its card before sharing the epk).
                commands.push(Command::RelayEscrowCheck {
                    gate_hash: escrow_gate,
                    suggested_interval_ms: ESCROW_POLL_INTERVAL_MS,
                });
                self.escrow_keys = Some(keys);
                self.state = LinkInitiatorState::Retrieving;
                self.pending.extend(commands);
            }
            Err(e) => {
                self.fail(LinkInitiatorFailureReason::HandshakeFailed {
                    detail: e.to_string(),
                });
            }
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            LinkInitiatorState::Finalized { .. } | LinkInitiatorState::Failed(_)
        )
    }

    fn fail(&mut self, reason: LinkInitiatorFailureReason) {
        self.state = LinkInitiatorState::Failed(reason);
        // Drop any queued commands — terminal-state cycles never emit.
        self.pending.clear();
    }
}
