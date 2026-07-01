// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! NFC Handshake Session
//!
//! Three-phase state machine for bidirectional encrypted NFC exchange.
//! Manages ephemeral key generation, X3DH symmetric key agreement,
//! encryption/decryption of card payloads, CRC16 validation, and
//! idempotency caching.
//!
//! ## Protocol
//!
//! Phase 1 (Initiator → Responder): Key offer (ExchangeNfc payload)
//! Phase 2 (Responder → Initiator): Key ack + encrypted card
//! Phase 3 (Initiator → Responder): Encrypted card response

use std::collections::HashMap;

use super::error::ExchangeError;
use super::nfc_active::ExchangeNfc;
use super::nfc_card_payload::NfcCardPayload;
use super::x3dh::X3DHKeyPair;
use crate::crypto::encryption::{self, SymmetricKey};
use crate::crypto::kdf::HKDF;
use crate::identity::Identity;

/// HKDF info prefix for NFC handshake key derivation.
const NFC_HANDSHAKE_INFO: &[u8] = b"vauchi-nfc-handshake-v1";

/// State of an NFC handshake session.
#[derive(Debug)]
#[non_exhaustive]
pub enum NfcHandshakeState {
    /// Session created, no action taken yet.
    Idle,
    /// Initiator has sent key offer, awaiting responder's key ack + encrypted card.
    KeyOfferSent { exchange_id: [u8; 32] },
    /// Responder has received key offer, derived shared key, sent ack + encrypted card.
    KeyAckReceived { exchange_id: [u8; 32] },
    /// Initiator has sent encrypted card response (final phase).
    PayloadSent { exchange_id: [u8; 32] },
    /// Exchange completed successfully.
    Complete {
        local_card: NfcCardPayload,
        remote_card: NfcCardPayload,
    },
    /// Exchange failed.
    Failed { reason: ExchangeError },
    /// Tap dropped after key exchange — fall back to relay.
    RelayFallback { exchange_id: [u8; 32] },
}

/// Result of a completed NFC exchange.
#[derive(Debug, Clone)]
pub struct NfcExchangeResult {
    pub local_card: NfcCardPayload,
    pub remote_card: NfcCardPayload,
    pub shared_key: SymmetricKey,
}

/// Manages the three-phase NFC encrypted handshake.
pub struct NfcHandshakeSession {
    state: NfcHandshakeState,
    our_x3dh: X3DHKeyPair,
    our_display_name: String,
    our_identity_key: [u8; 32],
    shared_key: Option<SymmetricKey>,
    their_card: Option<NfcCardPayload>,
    their_identity_key: Option<[u8; 32]>,
    completed_cache: HashMap<[u8; 32], NfcExchangeResult>,
}

impl NfcHandshakeSession {
    /// Creates a new initiator session (reader side).
    pub fn new_initiator(identity: &Identity, display_name: String) -> Self {
        Self {
            state: NfcHandshakeState::Idle,
            our_x3dh: X3DHKeyPair::generate(),
            our_display_name: display_name,
            our_identity_key: *identity.signing_public_key(),
            shared_key: None,
            their_card: None,
            their_identity_key: None,
            completed_cache: HashMap::new(),
        }
    }

    /// Creates a new responder session (HCE side).
    pub fn new_responder(identity: &Identity, display_name: String) -> Self {
        Self::new_initiator(identity, display_name)
    }

    /// Returns the current state.
    pub fn state(&self) -> &NfcHandshakeState {
        &self.state
    }

    /// Returns our X3DH public key for relay fallback message creation.
    pub fn our_exchange_key(&self) -> &[u8; 32] {
        self.our_x3dh.public_key()
    }

    /// Returns our identity key.
    pub fn our_identity_key(&self) -> &[u8; 32] {
        &self.our_identity_key
    }

    /// Phase 1 (Initiator): Create key offer payload.
    ///
    /// Generates a fresh ExchangeNfc payload containing our ephemeral X25519
    /// public key and identity key.
    pub fn create_key_offer(
        &mut self,
        identity: &Identity,
        now: u64,
    ) -> Result<Vec<u8>, ExchangeError> {
        if !matches!(self.state, NfcHandshakeState::Idle) {
            return Err(ExchangeError::InvalidState(
                "Expected Idle state for key offer".into(),
            ));
        }

        let nfc_payload = ExchangeNfc::generate(identity, &self.our_x3dh, now);
        let exchange_id = *nfc_payload.token();
        let bytes = nfc_payload.to_bytes();

        self.state = NfcHandshakeState::KeyOfferSent { exchange_id };
        Ok(bytes.to_vec())
    }

    /// Phase 2 (Responder): Process key offer, return key ack + encrypted card.
    ///
    /// Receives the initiator's ExchangeNfc payload, derives the shared key
    /// via symmetric DH + HKDF, encrypts our card payload.
    /// Returns (our ExchangeNfc bytes, encrypted card bytes).
    pub fn process_key_offer(
        &mut self,
        identity: &Identity,
        their_offer_bytes: &[u8],
        now: u64,
    ) -> Result<(Vec<u8>, Vec<u8>), ExchangeError> {
        if !matches!(self.state, NfcHandshakeState::Idle) {
            return Err(ExchangeError::InvalidState(
                "Expected Idle state for processing key offer".into(),
            ));
        }

        let their_nfc = ExchangeNfc::from_bytes(their_offer_bytes)?;
        verify_peer_payload(&their_nfc, now)?;

        // Self-exchange check (mirror of `ble_handshake.rs::process_key_offer:340`).
        // Without this, a reflection attack — an attacker replaying the
        // responder's own offer back to it — reaches `derive_symmetric_key`
        // and only fails later at AEAD decryption, instead of being rejected
        // at the identity layer the way BLE already does. Closes F-HIGH-3
        // of `_private/docs/audit-review-frameworks/results/2026-05-21-02-protocol-security-review.md`.
        if their_nfc.identity_key() == &self.our_identity_key {
            return Err(ExchangeError::SelfExchange);
        }
        // Remember the initiator's identity so phase 3's decrypt can bind AAD.
        self.their_identity_key = Some(*their_nfc.identity_key());

        let exchange_id = *their_nfc.token();

        if self.completed_cache.contains_key(&exchange_id) {
            return Err(ExchangeError::InvalidState(
                "Exchange already processed".into(),
            ));
        }

        // Symmetric DH: our_ephemeral x their_ephemeral
        let shared_key = derive_symmetric_key(
            &self.our_x3dh,
            &self.our_identity_key,
            their_nfc.identity_key(),
            their_nfc.exchange_key(),
            &exchange_id,
        )?;

        let our_nfc = ExchangeNfc::generate(identity, &self.our_x3dh, now);
        let our_nfc_bytes = our_nfc.to_bytes().to_vec();

        let encrypted = encrypt_card(
            &shared_key,
            &self.our_identity_key,
            their_nfc.identity_key(),
            &exchange_id,
            &self.our_display_name,
            self.our_x3dh.public_key(),
        )?;

        self.shared_key = Some(shared_key);
        self.state = NfcHandshakeState::KeyAckReceived { exchange_id };

        Ok((our_nfc_bytes, encrypted))
    }

    /// Phase 2 (Initiator): Process key ack + encrypted card from responder.
    ///
    /// Derives the shared key, decrypts and validates the responder's card.
    /// Returns our encrypted card for Phase 3.
    pub fn process_key_ack(
        &mut self,
        their_ack_bytes: &[u8],
        their_encrypted_card: &[u8],
        now: u64,
    ) -> Result<Vec<u8>, ExchangeError> {
        let exchange_id = match &self.state {
            NfcHandshakeState::KeyOfferSent { exchange_id } => *exchange_id,
            _ => {
                return Err(ExchangeError::InvalidState(
                    "Expected KeyOfferSent state".into(),
                ));
            }
        };

        let their_nfc = ExchangeNfc::from_bytes(their_ack_bytes)?;
        verify_peer_payload(&their_nfc, now)?;

        // W-1: reject a reflected own-offer replayed as the ack at the identity
        // layer — parity with `process_key_offer` and BLE (security review
        // 2026-07-01). Without this it fails only later at AEAD.
        if their_nfc.identity_key() == &self.our_identity_key {
            return Err(ExchangeError::SelfExchange);
        }

        // Symmetric DH: our_ephemeral x their_ephemeral
        let shared_key = derive_symmetric_key(
            &self.our_x3dh,
            &self.our_identity_key,
            their_nfc.identity_key(),
            their_nfc.exchange_key(),
            &exchange_id,
        )?;

        let their_card = decrypt_and_validate_card(
            &shared_key,
            their_encrypted_card,
            their_nfc.identity_key(),
            &self.our_identity_key,
            &exchange_id,
        )?;

        // Encrypt our card with same key, fresh nonce
        let encrypted = encrypt_card(
            &shared_key,
            &self.our_identity_key,
            their_nfc.identity_key(),
            &exchange_id,
            &self.our_display_name,
            self.our_x3dh.public_key(),
        )?;

        self.shared_key = Some(shared_key);
        self.their_card = Some(their_card);
        self.state = NfcHandshakeState::PayloadSent { exchange_id };

        Ok(encrypted)
    }

    /// Phase 3 (Responder): Process encrypted card from initiator.
    ///
    /// Decrypts the initiator's card, validates CRC16, and completes the exchange.
    pub fn process_encrypted_card(
        &mut self,
        their_encrypted_card: &[u8],
    ) -> Result<NfcExchangeResult, ExchangeError> {
        let exchange_id = match &self.state {
            NfcHandshakeState::KeyAckReceived { exchange_id } => *exchange_id,
            _ => {
                return Err(ExchangeError::InvalidState(
                    "Expected KeyAckReceived state".into(),
                ));
            }
        };

        let shared_key = self
            .shared_key
            .as_ref()
            .ok_or_else(|| ExchangeError::InvalidState("No shared key".into()))?;
        let their_identity = self
            .their_identity_key
            .ok_or_else(|| ExchangeError::InvalidState("No peer identity".into()))?;

        let their_card = decrypt_and_validate_card(
            shared_key,
            their_encrypted_card,
            &their_identity,
            &self.our_identity_key,
            &exchange_id,
        )?;

        let our_card = NfcCardPayload::new(
            self.our_identity_key,
            self.our_display_name.clone(),
            *self.our_x3dh.public_key(),
        );

        let result = NfcExchangeResult {
            local_card: our_card,
            remote_card: their_card,
            shared_key: shared_key.clone(),
        };

        self.completed_cache.insert(exchange_id, result.clone());
        self.state = NfcHandshakeState::Complete {
            local_card: result.local_card.clone(),
            remote_card: result.remote_card.clone(),
        };

        Ok(result)
    }

    /// Marks the session as complete for the initiator after Phase 3 send is confirmed.
    pub fn confirm_send_success(&mut self) -> Result<NfcExchangeResult, ExchangeError> {
        let exchange_id = match &self.state {
            NfcHandshakeState::PayloadSent { exchange_id } => *exchange_id,
            _ => {
                return Err(ExchangeError::InvalidState(
                    "Expected PayloadSent state".into(),
                ));
            }
        };

        let their_card = self
            .their_card
            .take()
            .ok_or_else(|| ExchangeError::InvalidState("No peer card".into()))?;
        let shared_key = self
            .shared_key
            .as_ref()
            .ok_or_else(|| ExchangeError::InvalidState("No shared key".into()))?;

        let our_card = NfcCardPayload::new(
            self.our_identity_key,
            self.our_display_name.clone(),
            *self.our_x3dh.public_key(),
        );

        let result = NfcExchangeResult {
            local_card: our_card,
            remote_card: their_card,
            shared_key: shared_key.clone(),
        };

        self.completed_cache.insert(exchange_id, result.clone());
        self.state = NfcHandshakeState::Complete {
            local_card: result.local_card.clone(),
            remote_card: result.remote_card.clone(),
        };

        Ok(result)
    }

    /// Transitions to relay fallback state.
    ///
    /// Called when the tap drops after key exchange but before card exchange.
    pub fn enter_relay_fallback(&mut self) -> Result<([u8; 32], SymmetricKey), ExchangeError> {
        let exchange_id = match &self.state {
            NfcHandshakeState::KeyOfferSent { exchange_id }
            | NfcHandshakeState::KeyAckReceived { exchange_id }
            | NfcHandshakeState::PayloadSent { exchange_id } => *exchange_id,
            _ => {
                return Err(ExchangeError::InvalidState(
                    "Cannot enter relay fallback from current state".into(),
                ));
            }
        };

        let shared_key = self
            .shared_key
            .as_ref()
            .ok_or_else(|| ExchangeError::InvalidState("No shared key for relay fallback".into()))?
            .clone();

        self.state = NfcHandshakeState::RelayFallback { exchange_id };

        Ok((exchange_id, shared_key))
    }
}

impl Drop for NfcHandshakeSession {
    fn drop(&mut self) {
        // SymmetricKey has ZeroizeOnDrop; clear our Option explicitly
        self.shared_key = None;
    }
}

/// Verifies a peer offer/ack: not expired, and carries a valid Ed25519
/// signature. Single choke point for both handshake entry points — the
/// signature is what authenticates each ephemeral key to its identity.
///
/// INVARIANT: callers MUST run this before `derive_symmetric_key`. The KDF
/// identity binding (F-HIGH-2) is defence-in-depth on top of this signature
/// check, not a replacement for it (security review C-1, 2026-07-01).
fn verify_peer_payload(their_nfc: &ExchangeNfc, now: u64) -> Result<(), ExchangeError> {
    if their_nfc.is_expired(now) {
        return Err(ExchangeError::NfcExpired);
    }
    if !their_nfc.verify_signature() {
        return Err(ExchangeError::InvalidSignature);
    }
    Ok(())
}

/// Derives the session key from an ephemeral X25519 DH, with the HKDF `info`
/// binding BOTH identities and BOTH ephemerals (UKS resistance, F-HIGH-2).
///
/// INVARIANT: `verify_peer_payload` MUST have accepted the peer offer/ack
/// first — the Ed25519 signature is what authenticates each ephemeral to its
/// identity; this binding is defence-in-depth on top (review C-1).
fn derive_symmetric_key(
    our_keys: &X3DHKeyPair,
    our_identity: &[u8; 32],
    their_identity: &[u8; 32],
    their_pub: &[u8; 32],
    exchange_id: &[u8; 32],
) -> Result<SymmetricKey, ExchangeError> {
    let dh_secret = our_keys.diffie_hellman(their_pub)?;

    let our_pub = our_keys.public_key();
    let info = build_hkdf_info(
        our_identity,
        our_pub,
        their_identity,
        their_pub,
        exchange_id,
    );
    let derived = HKDF::derive_key(None, &*dh_secret, &info);
    Ok(SymmetricKey::from_bytes(*derived))
}

/// Builds the HKDF `info`, binding each party's identity to its ephemeral.
///
/// Pairs `identity || ephemeral` per party, then sorts the two 64-byte pairs
/// so both sides derive the same key. Pairing (not sorting a flat key list)
/// binds identity to ephemeral, so a peer with the same ephemeral but a
/// different identity yields a different key — UKS resistance (F-HIGH-2,
/// ADR-007).
fn build_hkdf_info(
    our_identity: &[u8; 32],
    our_eph: &[u8; 32],
    their_identity: &[u8; 32],
    their_eph: &[u8; 32],
    exchange_id: &[u8; 32],
) -> Vec<u8> {
    let mut ours = Vec::with_capacity(64);
    ours.extend_from_slice(our_identity);
    ours.extend_from_slice(our_eph);
    let mut theirs = Vec::with_capacity(64);
    theirs.extend_from_slice(their_identity);
    theirs.extend_from_slice(their_eph);

    let mut info = NFC_HANDSHAKE_INFO.to_vec();
    // Sort the two pairs so both sides derive the same key.
    if ours <= theirs {
        info.extend_from_slice(&ours);
        info.extend_from_slice(&theirs);
    } else {
        info.extend_from_slice(&theirs);
        info.extend_from_slice(&ours);
    }
    info.extend_from_slice(exchange_id);
    info
}

/// Encrypts a card payload with the shared key, binding the exchange
/// identities into the AEAD associated data (W-2, parity with BLE).
fn encrypt_card(
    key: &SymmetricKey,
    sender_identity: &[u8; 32],
    receiver_identity: &[u8; 32],
    exchange_id: &[u8; 32],
    display_name: &str,
    exchange_key: &[u8; 32],
) -> Result<Vec<u8>, ExchangeError> {
    let payload = NfcCardPayload::new(*sender_identity, display_name.to_string(), *exchange_key);
    let plaintext = payload
        .to_bytes()
        .map_err(|_| ExchangeError::SerializationFailed)?;
    let aad = build_card_aad(sender_identity, receiver_identity, exchange_id);
    encryption::encrypt_with_ad(key, &plaintext, &aad).map_err(|_| ExchangeError::CryptoError)
}

/// Decrypts and validates a card payload, requiring the AEAD associated data
/// to match the exchange identities the sender bound (W-2).
fn decrypt_and_validate_card(
    key: &SymmetricKey,
    ciphertext: &[u8],
    sender_identity: &[u8; 32],
    receiver_identity: &[u8; 32],
    exchange_id: &[u8; 32],
) -> Result<NfcCardPayload, ExchangeError> {
    let aad = build_card_aad(sender_identity, receiver_identity, exchange_id);
    let plaintext = encryption::decrypt_with_ad(key, ciphertext, &aad)
        .map_err(|_| ExchangeError::NfcDecryptionFailed)?;
    let card =
        NfcCardPayload::from_bytes(&plaintext).map_err(|_| ExchangeError::SerializationFailed)?;

    if !card.verify_crc16() {
        return Err(ExchangeError::NfcCrcMismatch);
    }

    Ok(card)
}

/// Builds the AEAD associated data for a card ciphertext: the sender's and
/// receiver's identity keys plus the session `exchange_id`. Binds each card
/// to who sent it, to whom, and in which session (W-2, mirrors BLE's AAD).
fn build_card_aad(sender: &[u8; 32], receiver: &[u8; 32], exchange_id: &[u8; 32]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(96);
    aad.extend_from_slice(sender);
    aad.extend_from_slice(receiver);
    aad.extend_from_slice(exchange_id);
    aad
}

// INLINE_TEST_REQUIRED: build_hkdf_info is a private fn; the UKS
// identity-binding property (F-HIGH-2) must be asserted against the actual
// HKDF derivation input, which no integration test in tests/ can reach.
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Both peers pair (identity || ephemeral) and sort the two pairs, so they
    // derive identical HKDF info regardless of who is offer vs ack.
    // @internal
    #[test]
    fn hkdf_info_symmetric_across_peers() {
        let (a_id, a_eph) = ([1u8; 32], [2u8; 32]);
        let (b_id, b_eph) = ([3u8; 32], [4u8; 32]);
        let eid = [9u8; 32];
        assert_eq!(
            build_hkdf_info(&a_id, &a_eph, &b_id, &b_eph, &eid),
            build_hkdf_info(&b_id, &b_eph, &a_id, &a_eph, &eid),
        );
    }

    // UKS resistance (F-HIGH-2): same ephemerals, different peer identity must
    // NOT collide — the key is bound to the identities, not just the ephemerals.
    // @internal
    #[test]
    fn hkdf_info_binds_identity_uks_resistance() {
        let (a_id, a_eph) = ([1u8; 32], [2u8; 32]);
        let (b_id, b_eph) = ([3u8; 32], [4u8; 32]);
        let c_id = [5u8; 32];
        let eid = [9u8; 32];
        assert_ne!(
            build_hkdf_info(&a_id, &a_eph, &b_id, &b_eph, &eid),
            build_hkdf_info(&a_id, &a_eph, &c_id, &b_eph, &eid),
            "same ephemerals with a different identity must yield a different key",
        );
    }

    proptest! {
        // Symmetry holds for arbitrary keys (CC-04).
        // @internal
        #[test]
        fn hkdf_info_symmetric_prop(
            a_id in any::<[u8; 32]>(),
            a_eph in any::<[u8; 32]>(),
            b_id in any::<[u8; 32]>(),
            b_eph in any::<[u8; 32]>(),
            eid in any::<[u8; 32]>(),
        ) {
            prop_assert_eq!(
                build_hkdf_info(&a_id, &a_eph, &b_id, &b_eph, &eid),
                build_hkdf_info(&b_id, &b_eph, &a_id, &a_eph, &eid),
            );
        }
    }

    // W-2: a card ciphertext is bound to the exchange identities via AAD;
    // decrypting with a different sender identity fails at the AEAD tag.
    // @internal
    #[test]
    fn card_aad_binds_identities() {
        let key = SymmetricKey::from_bytes([7u8; 32]);
        let (sender, receiver, eid, eph) = ([1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]);

        let ct = encrypt_card(&key, &sender, &receiver, &eid, "Alice", &eph).unwrap();

        let card = decrypt_and_validate_card(&key, &ct, &sender, &receiver, &eid)
            .expect("matching AAD must decrypt");
        assert_eq!(card.identity_key, sender);

        let wrong = decrypt_and_validate_card(&key, &ct, &receiver, &receiver, &eid);
        assert!(
            matches!(wrong, Err(ExchangeError::NfcDecryptionFailed)),
            "mismatched sender identity in AAD must fail, got {:?}",
            wrong
        );
    }
}
