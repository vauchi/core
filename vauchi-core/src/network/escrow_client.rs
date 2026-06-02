// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Escrow client — the pure request/response mapping for core-owned
//! relay-escrow exchange (ADR-049).
//!
//! Core (not the frontend) drives the relay round-trip for link-mode and
//! reciprocity escrow. The state machines already emit
//! `Command::RelayEscrow{Deposit,Check,Retrieve}` with raw-byte gate/slot
//! hashes and an encrypted card blob; this module turns those into the
//! wire [`EscrowMessage`] and interprets the relay's [`EscrowResponse`]
//! into a transport-agnostic [`EscrowOutcome`].
//!
//! The OHTTP transport that performs the round-trip is layered on top
//! (Phase 1 T2) — keeping the protocol semantics independently testable.
//!
//! ## Wire encoding
//!
//! Gate and slot hashes are hex-encoded (the relay matches gates by
//! `hex::decode`); the blob is base64 (`URL_SAFE_NO_PAD`, matching
//! `link_mode`). The relay stores the blob as an opaque string and never
//! decodes it, so the only constraint is that depositor and retriever —
//! both this code — agree on the encoding.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use vauchi_protocol::escrow::{EscrowMessage, EscrowResponse, MAX_SLOTS_PER_GATE};

use crate::platform::Command;

/// The result of one escrow round-trip, independent of which gate it
/// queried. The caller pairs this with the request's `gate_hash` to emit
/// the matching `Event::RelayEscrow*`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscrowOutcome {
    /// A `Put` was accepted — stored, or already present (idempotent).
    Deposited,
    /// The gate is not ready / the slot holds no blob yet — keep polling.
    Pending,
    /// Both slots are filled: the peer has deposited. Drives
    /// `Event::RelayEscrowReady`.
    Ready,
    /// A blob was retrieved. Drives `Event::RelayEscrowBlobReceived`.
    Retrieved(Vec<u8>),
    /// The round-trip failed terminally. Drives
    /// `Event::RelayEscrowFailed` with this stable reason id.
    Failed(String),
}

/// Build the relay [`EscrowMessage`] for an escrow [`Command`].
///
/// Returns `None` for any non-escrow command, so a caller can pass every
/// pending command through unconditionally.
pub fn escrow_request(command: &Command) -> Option<EscrowMessage> {
    match command {
        Command::RelayEscrowDeposit {
            gate_hash,
            slot_hash,
            encrypted_card,
            ttl_seconds,
        } => Some(EscrowMessage::Put {
            gate_hash: hex::encode(gate_hash),
            slot_hash: hex::encode(slot_hash),
            blob: URL_SAFE_NO_PAD.encode(encrypted_card),
            ttl_seconds: *ttl_seconds,
        }),
        Command::RelayEscrowCheck { gate_hash, .. } => Some(EscrowMessage::Count {
            gate_hash: hex::encode(gate_hash),
        }),
        Command::RelayEscrowRetrieve {
            gate_hash,
            slot_hash,
        } => Some(EscrowMessage::Get {
            gate_hash: hex::encode(gate_hash),
            slot_hash: hex::encode(slot_hash),
        }),
        _ => None,
    }
}

/// Interpret a relay [`EscrowResponse`] into a transport-agnostic
/// [`EscrowOutcome`].
pub fn escrow_outcome(response: &EscrowResponse) -> EscrowOutcome {
    match response {
        EscrowResponse::Stored | EscrowResponse::AlreadyExists => EscrowOutcome::Deposited,
        EscrowResponse::Count { count } | EscrowResponse::NotReady { count } => {
            if *count >= MAX_SLOTS_PER_GATE {
                EscrowOutcome::Ready
            } else {
                EscrowOutcome::Pending
            }
        }
        EscrowResponse::Blob { blob } => match URL_SAFE_NO_PAD.decode(blob) {
            Ok(bytes) => EscrowOutcome::Retrieved(bytes),
            Err(_) => EscrowOutcome::Failed("malformed_blob".to_string()),
        },
        EscrowResponse::GateFull => EscrowOutcome::Failed("gate_full".to_string()),
        EscrowResponse::BlobTooLarge => EscrowOutcome::Failed("blob_too_large".to_string()),
        EscrowResponse::NotFound => EscrowOutcome::Failed("not_found".to_string()),
    }
}
