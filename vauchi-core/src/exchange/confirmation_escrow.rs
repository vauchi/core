// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Confirmation escrow key derivation.
//!
//! Derives gate and slot hashes for the reciprocity confirmation
//! escrow. Uses distinct HKDF domains from the card escrow
//! (ADR-007 compliant, no domain collision).

use sha2::{Digest, Sha256};

use super::escrow::EscrowRole;
use crate::crypto::kdf::HKDF;

const DOMAIN_CONFIRM_GATE: &[u8] = b"vauchi-confirm-escrow-gate-v1";
const DOMAIN_CONFIRM_SLOT_INIT: &[u8] = b"vauchi-confirm-escrow-slot-init-v1";
const DOMAIN_CONFIRM_SLOT_RESP: &[u8] = b"vauchi-confirm-escrow-slot-resp-v1";

/// Derived confirmation escrow identifiers.
///
/// Unlike `EscrowKeys`, this has no `card_key` — confirmation
/// deposits are raw pseudorandom tokens, not encrypted cards.
pub struct ConfirmationEscrowKeys {
    /// Gate identifier (hex-encoded SHA-256).
    pub gate_hash: String,
    /// Our slot hash (hex-encoded).
    pub our_slot: String,
    /// Their slot hash (hex-encoded).
    pub their_slot: String,
}

impl ConfirmationEscrowKeys {
    /// Derive confirmation escrow keys from a shared secret and role.
    ///
    /// Role for symmetric QR exchanges: sort identity public keys
    /// lexicographically — smaller key gets `Initiator`.
    pub fn derive(shared_secret: &[u8], role: EscrowRole) -> Self {
        let gate_raw = HKDF::derive_key(None, shared_secret, DOMAIN_CONFIRM_GATE);
        let slot_init_raw = HKDF::derive_key(None, shared_secret, DOMAIN_CONFIRM_SLOT_INIT);
        let slot_resp_raw = HKDF::derive_key(None, shared_secret, DOMAIN_CONFIRM_SLOT_RESP);

        let gate_hash = hex::encode(Sha256::digest(gate_raw.as_ref()));
        let slot_init = hex::encode(Sha256::digest(slot_init_raw.as_ref()));
        let slot_resp = hex::encode(Sha256::digest(slot_resp_raw.as_ref()));

        let (our_slot, their_slot) = match role {
            EscrowRole::Initiator => (slot_init, slot_resp),
            EscrowRole::Responder => (slot_resp, slot_init),
        };

        Self {
            gate_hash,
            our_slot,
            their_slot,
        }
    }
}
