// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reciprocity confirmation cascade driver.
//!
//! Drives relay escrow deposit → poll → retrieve → verify.
//! Falls through to `Pending` when escrow is exhausted (network
//! failure or timeout), letting relay sync confirm later.
//!
//! Sub-component of `ExchangeEngine` (ADR-031). Not a standalone
//! `WorkflowEngine` — it emits `Command`s and receives
//! `Event`s via the engine.

use serde::{Deserialize, Serialize};
use vauchi_core::exchange::reciprocity::Reciprocity;
use vauchi_core::{Command, Event};
use zeroize::Zeroize;

/// Confirmation cascade level (Phase 1: escrow only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CascadeLevel {
    /// Relay escrow deposit/poll (Level 3 in design spec).
    RelayEscrow,
    /// Exhausted — waiting for relay sync (Level 4).
    Pending,
    /// Confirmed via escrow.
    Done,
}

/// Persisted confirmation state for crash recovery (design spec §5.1).
///
/// Encrypted and stored in the contact's `confirmation_state` BLOB column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationState {
    pub our_token: [u8; 32],
    pub expected_their_token: [u8; 32],
    pub gate_hash: String,
    pub our_slot: String,
    pub their_slot: String,
    pub deposit_sent: bool,
}

/// Drives the reciprocity confirmation cascade.
pub struct ReciprocityConfirmer {
    our_token: [u8; 32],
    expected_their_token: [u8; 32],
    escrow_gate: String,
    escrow_our_slot: String,
    escrow_their_slot: String,
    exchange_timestamp: u64,
    level: CascadeLevel,
    deposit_retry_count: u32,
    deposit_sent: bool,
}

impl Drop for ReciprocityConfirmer {
    fn drop(&mut self) {
        self.our_token.zeroize();
        self.expected_their_token.zeroize();
    }
}

const MAX_DEPOSIT_RETRIES: u32 = 3;
const ESCROW_TTL_SECONDS: u32 = 7 * 24 * 3600; // 7 days
const ESCROW_POLL_INTERVAL_MS: u32 = 30_000; // 30 seconds initial

impl ReciprocityConfirmer {
    /// Create a new confirmer with tokens and escrow keys from key agreement.
    pub fn new(
        our_token: [u8; 32],
        expected_their_token: [u8; 32],
        escrow_gate: String,
        escrow_our_slot: String,
        escrow_their_slot: String,
        exchange_timestamp: u64,
        _has_internet: bool,
    ) -> Self {
        Self {
            our_token,
            expected_their_token,
            escrow_gate,
            escrow_our_slot,
            escrow_their_slot,
            exchange_timestamp,
            level: CascadeLevel::RelayEscrow,
            deposit_retry_count: 0,
            deposit_sent: false,
        }
    }

    /// Resume from persisted state (crash recovery — skip to escrow level).
    pub fn from_persisted(state: ConfirmationState, exchange_timestamp: u64) -> Self {
        Self {
            our_token: state.our_token,
            expected_their_token: state.expected_their_token,
            escrow_gate: state.gate_hash,
            escrow_our_slot: state.our_slot,
            escrow_their_slot: state.their_slot,
            exchange_timestamp,
            level: CascadeLevel::RelayEscrow,
            deposit_retry_count: 0,
            deposit_sent: state.deposit_sent,
        }
    }

    /// Returns initial commands to start the cascade.
    pub fn start(&mut self) -> Vec<Command> {
        match self.level {
            CascadeLevel::RelayEscrow => {
                if self.deposit_sent {
                    // Resuming after crash — skip straight to polling
                    vec![self.make_check_command()]
                } else {
                    vec![self.make_deposit_command()]
                }
            }
            _ => Vec::new(),
        }
    }

    /// Process a hardware event. Returns commands to emit.
    pub fn handle_event(&mut self, event: &Event) -> Vec<Command> {
        match event {
            Event::RelayEscrowReady { gate_hash } if *gate_hash == self.gate_bytes() => {
                // Gate has ≥2 deposits — retrieve their blob
                vec![self.make_retrieve_command()]
            }
            Event::RelayEscrowBlobReceived { gate_hash, blob }
                if *gate_hash == self.gate_bytes() =>
            {
                self.handle_blob_received(blob);
                Vec::new()
            }
            Event::RelayEscrowFailed { gate_hash, .. } if *gate_hash == self.gate_bytes() => {
                self.handle_escrow_failed()
            }
            _ => Vec::new(),
        }
    }

    /// Current reciprocity result.
    pub fn reciprocity(&self) -> Reciprocity {
        match self.level {
            CascadeLevel::Done => Reciprocity::Confirmed,
            CascadeLevel::Pending => Reciprocity::Pending,
            CascadeLevel::RelayEscrow => Reciprocity::Pending,
        }
    }

    /// Whether the confirmer has finished (confirmed or exhausted all levels).
    pub fn is_done(&self) -> bool {
        matches!(self.level, CascadeLevel::Done | CascadeLevel::Pending)
    }

    /// Build a `ConfirmationState` for persistence.
    pub fn to_persisted_state(&self) -> ConfirmationState {
        ConfirmationState {
            our_token: self.our_token,
            expected_their_token: self.expected_their_token,
            gate_hash: self.escrow_gate.clone(),
            our_slot: self.escrow_our_slot.clone(),
            their_slot: self.escrow_their_slot.clone(),
            deposit_sent: self.deposit_sent,
        }
    }

    /// The exchange timestamp for generation checks (spec §5.2).
    pub fn exchange_timestamp(&self) -> u64 {
        self.exchange_timestamp
    }

    // ── Private helpers ──

    fn gate_bytes(&self) -> Vec<u8> {
        hex::decode(&self.escrow_gate).unwrap_or_else(|_| self.escrow_gate.as_bytes().to_vec())
    }

    fn our_slot_bytes(&self) -> Vec<u8> {
        hex::decode(&self.escrow_our_slot)
            .unwrap_or_else(|_| self.escrow_our_slot.as_bytes().to_vec())
    }

    fn their_slot_bytes(&self) -> Vec<u8> {
        hex::decode(&self.escrow_their_slot)
            .unwrap_or_else(|_| self.escrow_their_slot.as_bytes().to_vec())
    }

    fn handle_blob_received(&mut self, blob: &[u8]) {
        use subtle::ConstantTimeEq;
        if blob.len() == 32 && bool::from(blob.ct_eq(self.expected_their_token.as_slice())) {
            self.level = CascadeLevel::Done;
        } else {
            // Invalid blob — fall through to pending (don't retry)
            self.level = CascadeLevel::Pending;
        }
    }

    fn handle_escrow_failed(&mut self) -> Vec<Command> {
        // Retry up to MAX_DEPOSIT_RETRIES on any escrow failure
        // (deposit rejected, poll timeout, network error).
        // deposit_sent is for crash recovery only (skip re-deposit on relaunch).
        self.deposit_retry_count += 1;
        if self.deposit_retry_count <= MAX_DEPOSIT_RETRIES {
            return vec![self.make_deposit_command()];
        }
        // Exhausted retries — fall through to pending (relay sync takes over)
        self.level = CascadeLevel::Pending;
        Vec::new()
    }

    fn make_deposit_command(&mut self) -> Command {
        self.deposit_sent = true;
        Command::RelayEscrowDeposit {
            gate_hash: self.gate_bytes(),
            slot_hash: self.our_slot_bytes(),
            encrypted_card: self.our_token.to_vec(),
            ttl_seconds: ESCROW_TTL_SECONDS,
        }
    }

    fn make_check_command(&self) -> Command {
        Command::RelayEscrowCheck {
            gate_hash: self.gate_bytes(),
            suggested_interval_ms: ESCROW_POLL_INTERVAL_MS,
        }
    }

    fn make_retrieve_command(&self) -> Command {
        Command::RelayEscrowRetrieve {
            gate_hash: self.gate_bytes(),
            slot_hash: self.their_slot_bytes(),
        }
    }
}
