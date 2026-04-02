// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Escrow key derivation and card encryption for relay-mediated exchange.
//!
//! Core derives hashes and encrypts cards, then emits `RelayEscrowDeposit`
//! commands. Frontends execute the relay call — core NEVER calls the relay
//! directly (ADR-031).
//!
//! All derivations use HKDF-SHA256 with unique domain separation strings
//! (ADR-002, ADR-007). Card encryption uses XChaCha20-Poly1305 (ADR-002).

use sha2::{Digest, Sha256};

use crate::crypto::encryption::{self, EncryptionError, SymmetricKey};
use crate::crypto::kdf::HKDF;

// =========================================================================
// Domain separation constants — MUST NOT collide with existing domains.
// See .claude/docs/ for the full domain registry.
// =========================================================================

const DOMAIN_GATE: &[u8] = b"vauchi-escrow-gate-v1";
const DOMAIN_SLOT_INIT: &[u8] = b"vauchi-escrow-slot-init-v1";
const DOMAIN_SLOT_RESP: &[u8] = b"vauchi-escrow-slot-resp-v1";
const DOMAIN_CARD_KEY: &[u8] = b"vauchi-escrow-card-key-v1";

/// Which role in the exchange (determines slot assignment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscrowRole {
    /// The party who initiated the exchange (e.g., generated the Link URL).
    Initiator,
    /// The party who responded (e.g., opened the Link URL).
    Responder,
}

/// Derived escrow keys and hashes for a single exchange.
///
/// Both parties derive identical `gate_hash` from the same shared secret.
/// Each party uses their own `our_slot` and the other's `their_slot`.
pub struct EscrowKeys {
    /// Gate identifier (hex-encoded SHA-256 of HKDF output).
    pub gate_hash: String,
    /// Our slot hash (hex-encoded).
    pub our_slot: String,
    /// Their slot hash (hex-encoded).
    pub their_slot: String,
    /// Symmetric key for card encryption (zeroized on drop).
    card_key: SymmetricKey,
}

impl EscrowKeys {
    /// Derive all escrow keys from a shared secret and role.
    ///
    /// The shared secret is typically the output of an X25519 ECDH.
    /// Both parties must use the same shared secret but opposite roles.
    pub fn derive(shared_secret: &[u8], role: EscrowRole) -> Self {
        // Derive raw 32-byte keys via HKDF with domain separation.
        let gate_raw = HKDF::derive_key(None, shared_secret, DOMAIN_GATE);
        let slot_init_raw = HKDF::derive_key(None, shared_secret, DOMAIN_SLOT_INIT);
        let slot_resp_raw = HKDF::derive_key(None, shared_secret, DOMAIN_SLOT_RESP);
        let card_key_raw = HKDF::derive_key(None, shared_secret, DOMAIN_CARD_KEY);

        // Hash the HKDF outputs to produce the relay-visible identifiers.
        // The relay sees only H(HKDF(...)), never the HKDF output itself.
        let gate_hash = hex::encode(Sha256::digest(gate_raw.as_ref()));
        let slot_init = hex::encode(Sha256::digest(slot_init_raw.as_ref()));
        let slot_resp = hex::encode(Sha256::digest(slot_resp_raw.as_ref()));

        let (our_slot, their_slot) = match role {
            EscrowRole::Initiator => (slot_init, slot_resp),
            EscrowRole::Responder => (slot_resp, slot_init),
        };

        let card_key = SymmetricKey::from_bytes(*card_key_raw);

        Self {
            gate_hash,
            our_slot,
            their_slot,
            card_key,
        }
    }

    /// Encrypt a serialized card for escrow deposit.
    ///
    /// Returns the ciphertext (nonce prepended) suitable for base64
    /// encoding and relay deposit.
    pub fn encrypt_card(&self, plaintext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        encryption::encrypt(&self.card_key, plaintext)
    }

    /// Decrypt a card blob retrieved from the escrow relay.
    pub fn decrypt_card(&self, ciphertext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        encryption::decrypt(&self.card_key, ciphertext)
    }
}
