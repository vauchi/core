// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure-Rust state machine driving the responder side of link-mode
//! contact exchange.
//!
//! Inputs are [`LinkResponderSession::apply_hardware_event`] (relay
//! events arriving from the cycle thread) and
//! [`LinkResponderSession::tick`] (wall-clock checks for the polling
//! deadline). Outputs are [`LinkResponderSession::drain_pending_commands`]
//! (commands the cycle thread should dispatch) and
//! [`LinkResponderSession::current_state`].
//!
//! The cycle thread (in `vauchi-platform`) wraps this state machine in
//! a thread-safe handle, dispatches the commands via the platform's
//! relay surface, and forwards events back. Keeping the state machine
//! pure Rust (no threads, no sleeps) lets it be exercised
//! deterministically by stateful property tests (CC-13) and by a
//! mock-relay integration test in `vauchi-platform/tests/it/`.
//!
//! See `_private/docs/problems/2026-04-27-deep-link-responder-flow/`
//! for the full design + risk register.

use std::time::Instant;

use crate::exchange::escrow::EscrowKeys;
use crate::exchange::link_mode::{LinkModeError, responder_complete};
use crate::platform::{Command, Event};

/// Suggested initial polling interval (ms) for the responder's
/// `RelayEscrowCheck`. Mirrors `exchange_link.rs::LINK_POLL_INTERVAL_MS`
/// — both sides poll on the same 30 s cadence, with relay-side backoff
/// capping at 5 min. Phase 0.1 of the implementation plan extracts both
/// constants to a shared module.
const RESPONDER_POLL_INTERVAL_MS: u32 = 30_000;

/// Why the responder cycle ended without a finalized contact.
///
/// Variants stable enough to drive distinct toasts and metrics.
/// `DecryptError` carries the underlying error message so logs are
/// useful.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkResponderFailureReason {
    /// Polling exhausted the deadline without a `RelayEscrowReady`
    /// event. The responder's encrypted card may still sit on the
    /// relay until its TTL elapses.
    PollingTimedOut,
    /// Card retrieved from escrow but symmetric decryption failed.
    /// Implies a corrupt slot write or a key-derivation drift between
    /// initiator and responder.
    DecryptError { detail: String },
    /// `RelayEscrowFailed` arrived for our gate. Most often indicates
    /// the responder's bootstrap deposit was rejected because the slot
    /// was already written (the same user re-opened a link they had
    /// previously accepted).
    DepositRejected,
    /// User-initiated cancel via the Polling screen's Cancel action
    /// (or the engine's `Drop` impl on navigate-back).
    Cancelled,
}

/// State of a link-mode responder cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkResponderState {
    /// Deposits dispatched, waiting for `RelayEscrowReady` on our gate
    /// or for the deadline to expire.
    Polling,
    /// `RelayEscrowReady` fired; `RelayEscrowRetrieve` dispatched;
    /// waiting for `RelayEscrowBlobReceived`.
    Retrieving,
    /// Blob retrieved + decrypted; carries the plaintext card-payload
    /// bytes the cycle thread hands to its persistence layer.
    Finalized { card_bytes: Vec<u8> },
    /// Terminal failure with typed reason.
    Failed(LinkResponderFailureReason),
}

/// Pure-Rust state machine for the responder side of link mode.
pub struct LinkResponderSession {
    state: LinkResponderState,
    keys: EscrowKeys,
    /// Wall-clock deadline for the polling phase. Once reached without
    /// `RelayEscrowReady`, transitions to `Failed(PollingTimedOut)`.
    poll_deadline: Instant,
    /// Bytes of `keys.gate_hash` decoded from hex once at construction
    /// time, so per-event comparisons are O(32) byte equality rather
    /// than re-decoding the hex string.
    gate_hash_bytes: Vec<u8>,
    /// Commands queued for `drain_pending_commands`. One-shot per call.
    pending: Vec<Command>,
}

impl LinkResponderSession {
    /// Construct a new responder session.
    ///
    /// `keys` and `deposit_commands` come from `responder_respond`
    /// (link_mode.rs). The session adds a `RelayEscrowCheck` so the
    /// relay starts watching the gate; the cycle thread drains all
    /// three commands on the first call to `drain_pending_commands`.
    ///
    /// `poll_deadline` is the wall-clock instant after which the
    /// session transitions to `Failed(PollingTimedOut)` if no
    /// `RelayEscrowReady` event has arrived.
    pub fn new(keys: EscrowKeys, deposit_commands: Vec<Command>, poll_deadline: Instant) -> Self {
        let gate_hash_bytes =
            hex::decode(&keys.gate_hash).expect("hex from hex::encode is always valid");

        let mut pending = deposit_commands;
        // Tell the relay to start watching the gate so the deposit
        // count crossing 2 fires `RelayEscrowReady`.
        pending.push(Command::RelayEscrowCheck {
            gate_hash: gate_hash_bytes.clone(),
            suggested_interval_ms: RESPONDER_POLL_INTERVAL_MS,
        });

        Self {
            state: LinkResponderState::Polling,
            keys,
            poll_deadline,
            gate_hash_bytes,
            pending,
        }
    }

    /// Current state — read-only accessor for the cycle thread + tests.
    pub fn current_state(&self) -> &LinkResponderState {
        &self.state
    }

    /// Drain queued commands. Idempotent: a second call returns empty.
    pub fn drain_pending_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.pending)
    }

    /// Apply a hardware event from the relay. Events for unrelated
    /// gates are ignored. Terminal states (`Finalized` / `Failed`)
    /// ignore all events — once a session has completed, further
    /// events do not unset its outcome.
    pub fn apply_hardware_event(&mut self, event: Event) {
        if self.is_terminal() {
            return;
        }

        match event {
            Event::RelayEscrowReady { gate_hash } => {
                if gate_hash != self.gate_hash_bytes {
                    return;
                }
                if matches!(self.state, LinkResponderState::Polling) {
                    self.transition_to_retrieving();
                }
            }
            Event::RelayEscrowFailed { gate_hash, .. } => {
                if gate_hash != self.gate_hash_bytes {
                    return;
                }
                self.fail(LinkResponderFailureReason::DepositRejected);
            }
            Event::RelayEscrowBlobReceived { gate_hash, blob } => {
                if gate_hash != self.gate_hash_bytes {
                    return;
                }
                if !matches!(self.state, LinkResponderState::Retrieving) {
                    return;
                }
                match responder_complete(&self.keys, &blob) {
                    Ok(card_bytes) => {
                        self.state = LinkResponderState::Finalized { card_bytes };
                    }
                    Err(LinkModeError::CardCryptoFailed(detail)) => {
                        self.fail(LinkResponderFailureReason::DecryptError { detail });
                    }
                    Err(other) => {
                        // responder_complete only emits CardCryptoFailed
                        // today, but stay defensive: any future error
                        // variant maps to DecryptError so the cycle
                        // thread always has a typed reason.
                        self.fail(LinkResponderFailureReason::DecryptError {
                            detail: other.to_string(),
                        });
                    }
                }
            }
            _ => {
                // Other hardware events (BLE / NFC / accelerometer /
                // direct transport / image picking) are not relevant
                // to the link-mode responder cycle.
            }
        }
    }

    /// Wall-clock deadline check. The cycle thread calls this on each
    /// poll iteration; tests pass arbitrary `Instant`s.
    pub fn tick(&mut self, now: Instant) {
        if self.is_terminal() {
            return;
        }
        if now >= self.poll_deadline && matches!(self.state, LinkResponderState::Polling) {
            self.fail(LinkResponderFailureReason::PollingTimedOut);
        }
    }

    /// User-initiated cancel. No-op if the session has already
    /// reached a terminal state.
    pub fn cancel(&mut self) {
        if self.is_terminal() {
            return;
        }
        self.fail(LinkResponderFailureReason::Cancelled);
    }

    /// Decoded gate-hash bytes — exposed for the cycle thread and tests
    /// so they can build `RelayEscrowReady` / `RelayEscrowBlobReceived`
    /// events without re-decoding the hex string.
    pub fn gate_hash_bytes(&self) -> Vec<u8> {
        self.gate_hash_bytes.clone()
    }

    /// Test-only helper: encrypt bytes with the session's card_key so
    /// tests can construct round-trip blobs without exposing the
    /// `EscrowKeys::encrypt_card` method through a non-test path.
    ///
    /// Marked `#[cfg(any(test, feature = "testing"))]` so it is not
    /// part of the production surface.
    #[cfg(any(test, feature = "testing"))]
    pub fn test_encrypt_card(
        &self,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, crate::crypto::encryption::EncryptionError> {
        self.keys.encrypt_card(plaintext)
    }

    fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            LinkResponderState::Finalized { .. } | LinkResponderState::Failed(_)
        )
    }

    fn transition_to_retrieving(&mut self) {
        self.state = LinkResponderState::Retrieving;
        self.pending.push(Command::RelayEscrowRetrieve {
            gate_hash: self.gate_hash_bytes.clone(),
            slot_hash: hex::decode(&self.keys.their_slot)
                .expect("hex from hex::encode is always valid"),
        });
    }

    fn fail(&mut self, reason: LinkResponderFailureReason) {
        self.state = LinkResponderState::Failed(reason);
        // Drop any queued commands — terminal-state cycles never emit.
        self.pending.clear();
    }
}
