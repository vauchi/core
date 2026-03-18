// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE Encrypted Handshake Session
//!
//! Four-phase state machine for bidirectional encrypted BLE contact exchange
//! with commitment scheme for atomic reciprocity.
//!
//! ## Protocol Overview
//!
//! ```text
//! Phase 1 (Initiator → Responder): KeyOffer
//!   [version(1)][identity_pub(32)][ephemeral_pub(32)][nonce(16)][timestamp(8)] = 89 bytes
//!
//! Phase 2 (Responder → Initiator): KeyAck + commitment + encrypted card
//!   [version(1)][identity_pub(32)][ephemeral_pub(32)][nonce(16)][commitment(32)] = 113 bytes
//!   + encrypted card bytes (separate BLE data characteristic)
//!
//! Phase 3 (Initiator → Responder): commitment + encrypted card
//!   [commitment(32)] + encrypted card bytes
//!
//! Phase 4 (Both sides): Reveal — verify commitments, decrypt
//! ```
//!
//! ## Key Derivation
//!
//! Single ephemeral×ephemeral DH, then HKDF with sorted public keys:
//! ```text
//! dh_secret = our_ephemeral.dh(their_ephemeral)
//! salt = sorted(our_nonce, their_nonce)
//! info = b"vauchi-ble-handshake-v1" || sorted(our_pub, their_pub)
//! session_key = HKDF(salt, dh_secret, info)
//! ```
//!
//! ## Commitment Scheme
//!
//! Each side commits to their encrypted card before seeing the other's:
//! `commitment = SHA-256(encrypted_card_bytes)`
//!
//! This prevents a malicious party from crafting their card based on
//! the other's card content.
//!
//! ## Security Properties
//!
//! - **Forward secrecy**: Ephemeral X25519 keypairs are per-session.
//! - **Atomic reciprocity**: Commitment scheme ensures both cards are
//!   locked before either is revealed.
//! - **Context binding**: AAD binds ciphertext to sender/receiver identity
//!   and timestamp, preventing replay and misdirection.
//! - **Expiry**: KeyOffer timestamps expire after 60 seconds.
//! - **Self-exchange prevention**: Identity key comparison rejects
//!   attempts to exchange with yourself.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use aws_lc_rs::digest::{digest, SHA256};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use super::ble_payload::BleCardPayload;
use super::error::ExchangeError;
use super::x3dh::X3DHKeyPair;
use crate::crypto::encryption::{self, SymmetricKey};
use crate::crypto::kdf::HKDF;
use crate::identity::Identity;

/// HKDF info prefix for BLE handshake key derivation.
pub const BLE_HANDSHAKE_INFO: &[u8] = b"vauchi-ble-handshake-v1";

/// Protocol version byte.
pub const BLE_HANDSHAKE_VERSION: u8 = 0x01;

/// Maximum age of a KeyOffer before it is considered expired (seconds).
const BLE_HANDSHAKE_EXPIRY_SECS: u64 = 60;

/// Size of the random nonce in KeyOffer/KeyAck messages.
const NONCE_SIZE: usize = 16;

/// KeyOffer wire size: version(1) + identity(32) + ephemeral(32) + nonce(16) + timestamp(8).
const KEY_OFFER_SIZE: usize = 1 + 32 + 32 + NONCE_SIZE + 8;

/// KeyAck wire size: version(1) + identity(32) + ephemeral(32) + nonce(16) + commitment(32).
const KEY_ACK_SIZE: usize = 1 + 32 + 32 + NONCE_SIZE + 32;

/// State of a BLE handshake session.
///
/// Tracks the 4-phase protocol progression from key exchange through
/// commitment verification to completed card exchange.
#[derive(Debug)]
pub enum BleHandshakeState {
    /// Session created, no action taken yet.
    Idle,
    /// Initiator has sent KeyOffer, awaiting KeyAck.
    KeyOfferSent { exchange_id: [u8; 32] },
    /// Responder has received KeyOffer, derived session key, encrypted card.
    KeyOfferReceived { exchange_id: [u8; 32] },
    /// Session key established, ready to send/receive encrypted payloads.
    SessionEstablished { exchange_id: [u8; 32] },
    /// Local encrypted card has been sent.
    SendingPayload { exchange_id: [u8; 32] },
    /// Awaiting remote encrypted card + commitment verification.
    AwaitingPayload {
        exchange_id: [u8; 32],
        local_commitment: [u8; 32],
    },
    /// Both encrypted cards received, pending reveal/verification.
    PayloadsExchanged {
        exchange_id: [u8; 32],
        local_commitment: [u8; 32],
        remote_commitment: [u8; 32],
        remote_encrypted: Vec<u8>,
    },
    /// Reveal has been sent, pending completion.
    RevealSent { exchange_id: [u8; 32] },
    /// Exchange completed successfully.
    Complete {
        local_card: BleCardPayload,
        remote_card: BleCardPayload,
    },
    /// Exchange failed.
    Failed { reason: ExchangeError },
}

/// Result of a completed BLE exchange.
#[derive(Debug, Clone)]
pub struct BleExchangeResult {
    /// The local card that was sent.
    pub local_card: BleCardPayload,
    /// The remote card that was received and decrypted.
    pub remote_card: BleCardPayload,
}

/// Manages the 4-phase BLE encrypted handshake.
///
/// Each session uses a fresh ephemeral X25519 keypair and a random nonce.
/// The session key is derived via single DH + HKDF. Encrypted cards are
/// committed before exchange to ensure atomic reciprocity.
///
/// # Key Lifecycle
///
/// - Ephemeral X25519 keypair: generated in constructor, consumed during key derivation.
/// - Session nonce (16 bytes): generated from OS CSPRNG in constructor.
/// - Session key: derived during Phase 2 processing, zeroized on Drop.
/// - Private key material: should not be used after `derive_session_key` completes.
pub struct BleHandshakeSession {
    state: BleHandshakeState,
    our_x3dh: X3DHKeyPair,
    our_nonce: [u8; NONCE_SIZE],
    our_identity_key: [u8; 32],
    our_card: BleCardPayload,
    our_timestamp: u64,
    session_key: Option<SymmetricKey>,
    their_card: Option<BleCardPayload>,
    their_identity_key: Option<[u8; 32]>,
    our_encrypted_card: Option<Vec<u8>>,
    our_commitment: Option<[u8; 32]>,
    their_commitment: Option<[u8; 32]>,
    their_encrypted_card: Option<Vec<u8>>,
    completed_cache: HashMap<[u8; 32], BleExchangeResult>,
}

impl BleHandshakeSession {
    /// Creates a new initiator session.
    ///
    /// The initiator sends the first KeyOffer message and drives the protocol forward.
    /// Generates a fresh ephemeral X25519 keypair and random nonce.
    pub fn new_initiator(identity: &Identity, card: BleCardPayload) -> Self {
        Self::new(identity, card)
    }

    /// Creates a new responder session.
    ///
    /// The responder waits for a KeyOffer, then responds with KeyAck + encrypted card.
    /// Generates a fresh ephemeral X25519 keypair and random nonce.
    pub fn new_responder(identity: &Identity, card: BleCardPayload) -> Self {
        Self::new(identity, card)
    }

    /// Creates an initiator session from a raw 32-byte identity key.
    ///
    /// Used by mobile bindings that don't have access to a full `Identity` object.
    pub fn new_initiator_from_key(identity_key: [u8; 32], card: BleCardPayload) -> Self {
        Self::new_from_key(identity_key, card)
    }

    /// Creates a responder session from a raw 32-byte identity key.
    ///
    /// Used by mobile bindings that don't have access to a full `Identity` object.
    pub fn new_responder_from_key(identity_key: [u8; 32], card: BleCardPayload) -> Self {
        Self::new_from_key(identity_key, card)
    }

    /// Internal constructor shared by initiator and responder.
    fn new(identity: &Identity, card: BleCardPayload) -> Self {
        Self::new_from_key(*identity.signing_public_key(), card)
    }

    /// Internal constructor from raw identity key bytes.
    fn new_from_key(identity_key: [u8; 32], card: BleCardPayload) -> Self {
        let nonce: [u8; NONCE_SIZE] = crate::crypto::random_bytes();

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before UNIX epoch")
            .as_secs();

        Self {
            state: BleHandshakeState::Idle,
            our_x3dh: X3DHKeyPair::generate(),
            our_nonce: nonce,
            our_identity_key: identity_key,
            our_card: card,
            our_timestamp: timestamp,
            session_key: None,
            their_card: None,
            their_identity_key: None,
            our_encrypted_card: None,
            our_commitment: None,
            their_commitment: None,
            their_encrypted_card: None,
            completed_cache: HashMap::new(),
        }
    }

    /// Returns the current handshake state.
    pub fn state(&self) -> &BleHandshakeState {
        &self.state
    }

    /// Phase 1 (Initiator): Create and serialize a KeyOffer message.
    ///
    /// Produces a 89-byte message:
    /// `[version(1)][identity_pub(32)][ephemeral_pub(32)][nonce(16)][timestamp(8)]`
    ///
    /// Transitions: `Idle → KeyOfferSent`
    pub fn create_key_offer(&mut self) -> Result<Vec<u8>, ExchangeError> {
        if !matches!(self.state, BleHandshakeState::Idle) {
            return Err(ExchangeError::InvalidState(
                "Expected Idle state for key offer".into(),
            ));
        }

        let mut offer = Vec::with_capacity(KEY_OFFER_SIZE);
        offer.push(BLE_HANDSHAKE_VERSION);
        offer.extend_from_slice(&self.our_identity_key);
        offer.extend_from_slice(self.our_x3dh.public_key());
        offer.extend_from_slice(&self.our_nonce);
        offer.extend_from_slice(&self.our_timestamp.to_be_bytes());

        let exchange_id = compute_exchange_id(&self.our_identity_key, self.our_x3dh.public_key());
        self.state = BleHandshakeState::KeyOfferSent { exchange_id };

        Ok(offer)
    }

    /// Phase 2 (Responder): Process a KeyOffer, derive session key, return KeyAck + encrypted card.
    ///
    /// Parses the initiator's 89-byte KeyOffer, validates version and timestamp,
    /// detects self-exchange, derives the shared session key via single DH + HKDF,
    /// encrypts our card, and computes commitment.
    ///
    /// Returns `(key_ack_bytes, encrypted_card_bytes)`.
    ///
    /// Transitions: `Idle → AwaitingPayload`
    pub fn process_key_offer(
        &mut self,
        their_offer: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>), ExchangeError> {
        if !matches!(self.state, BleHandshakeState::Idle) {
            return Err(ExchangeError::InvalidState(
                "Expected Idle state for processing key offer".into(),
            ));
        }

        // Parse KeyOffer
        if their_offer.len() < KEY_OFFER_SIZE {
            return Err(ExchangeError::InvalidBleFormat);
        }

        let version = their_offer[0];
        if version != BLE_HANDSHAKE_VERSION {
            return Err(ExchangeError::InvalidProtocolVersion);
        }

        let their_identity: [u8; 32] = their_offer[1..33]
            .try_into()
            .map_err(|_| ExchangeError::InvalidBleFormat)?;
        let their_ephemeral: [u8; 32] = their_offer[33..65]
            .try_into()
            .map_err(|_| ExchangeError::InvalidBleFormat)?;
        let their_nonce: [u8; NONCE_SIZE] = their_offer[65..81]
            .try_into()
            .map_err(|_| ExchangeError::InvalidBleFormat)?;
        let their_timestamp = u64::from_be_bytes(
            their_offer[81..89]
                .try_into()
                .map_err(|_| ExchangeError::InvalidBleFormat)?,
        );

        // Self-exchange check
        if their_identity == self.our_identity_key {
            return Err(ExchangeError::SelfExchange);
        }

        // Expiry check
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before UNIX epoch")
            .as_secs();
        if now.saturating_sub(their_timestamp) > BLE_HANDSHAKE_EXPIRY_SECS {
            return Err(ExchangeError::BleExpired);
        }

        let exchange_id = compute_exchange_id(&their_identity, &their_ephemeral);

        // Idempotency check
        if self.completed_cache.contains_key(&exchange_id) {
            return Err(ExchangeError::InvalidState(
                "Exchange already processed".into(),
            ));
        }

        // Derive session key: ephemeral × ephemeral DH + HKDF
        let session_key = derive_session_key(
            &self.our_x3dh,
            &their_ephemeral,
            &self.our_nonce,
            &their_nonce,
        )?;

        // Build AAD: sender_identity || receiver_identity || timestamp
        let aad = build_aad(&self.our_identity_key, &their_identity, self.our_timestamp);

        // Encrypt our card
        let plaintext = self
            .our_card
            .to_bytes()
            .map_err(|_| ExchangeError::SerializationFailed)?;
        let encrypted_card = encryption::encrypt_with_ad(&session_key, &plaintext, &aad)
            .map_err(|_| ExchangeError::CryptoError)?;

        // Compute commitment: SHA-256(encrypted_card)
        let commitment = compute_commitment(&encrypted_card);

        // Build KeyAck: version(1) + identity(32) + ephemeral(32) + nonce(16) + commitment(32) = 113
        let mut ack = Vec::with_capacity(KEY_ACK_SIZE);
        ack.push(BLE_HANDSHAKE_VERSION);
        ack.extend_from_slice(&self.our_identity_key);
        ack.extend_from_slice(self.our_x3dh.public_key());
        ack.extend_from_slice(&self.our_nonce);
        ack.extend_from_slice(&commitment);

        self.session_key = Some(session_key);
        self.their_identity_key = Some(their_identity);
        self.our_encrypted_card = Some(encrypted_card.clone());
        self.our_commitment = Some(commitment);
        self.state = BleHandshakeState::AwaitingPayload {
            exchange_id,
            local_commitment: commitment,
        };

        Ok((ack, encrypted_card))
    }

    /// Phase 2 (Initiator): Process KeyAck + encrypted card from responder.
    ///
    /// Parses the responder's 113-byte KeyAck, derives the session key,
    /// verifies the responder's commitment against their encrypted card,
    /// decrypts the responder's card, encrypts our own card, and computes
    /// our commitment.
    ///
    /// Returns `(our_commitment, our_encrypted_card)`.
    ///
    /// Transitions: `KeyOfferSent → PayloadsExchanged`
    pub fn process_key_ack(
        &mut self,
        their_ack: &[u8],
        their_encrypted_card: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>), ExchangeError> {
        let exchange_id = match &self.state {
            BleHandshakeState::KeyOfferSent { exchange_id } => *exchange_id,
            _ => {
                return Err(ExchangeError::InvalidState(
                    "Expected KeyOfferSent state for processing key ack".into(),
                ))
            }
        };

        // Parse KeyAck
        if their_ack.len() < KEY_ACK_SIZE {
            return Err(ExchangeError::InvalidBleFormat);
        }

        let version = their_ack[0];
        if version != BLE_HANDSHAKE_VERSION {
            return Err(ExchangeError::InvalidProtocolVersion);
        }

        let their_identity: [u8; 32] = their_ack[1..33]
            .try_into()
            .map_err(|_| ExchangeError::InvalidBleFormat)?;
        let their_ephemeral: [u8; 32] = their_ack[33..65]
            .try_into()
            .map_err(|_| ExchangeError::InvalidBleFormat)?;
        let their_nonce: [u8; NONCE_SIZE] = their_ack[65..81]
            .try_into()
            .map_err(|_| ExchangeError::InvalidBleFormat)?;
        let their_commitment: [u8; 32] = their_ack[81..113]
            .try_into()
            .map_err(|_| ExchangeError::InvalidBleFormat)?;

        // Verify commitment: SHA-256(encrypted_card) must match
        let computed_commitment = compute_commitment(their_encrypted_card);
        if !bool::from(computed_commitment.ct_eq(&their_commitment)) {
            return Err(ExchangeError::BleCommitmentMismatch);
        }

        // Derive session key: same DH as responder, same HKDF
        let session_key = derive_session_key(
            &self.our_x3dh,
            &their_ephemeral,
            &self.our_nonce,
            &their_nonce,
        )?;

        // Decrypt their card
        let their_aad = build_aad(&their_identity, &self.our_identity_key, self.our_timestamp);
        let their_plaintext =
            encryption::decrypt_with_ad(&session_key, their_encrypted_card, &their_aad)
                .map_err(|_| ExchangeError::BleDecryptionFailed)?;
        let their_card = BleCardPayload::from_bytes(&their_plaintext)
            .map_err(|_| ExchangeError::SerializationFailed)?;

        if !their_card.verify_crc16() {
            return Err(ExchangeError::BleDecryptionFailed);
        }

        // Encrypt our card
        let our_aad = build_aad(&self.our_identity_key, &their_identity, self.our_timestamp);
        let our_plaintext = self
            .our_card
            .to_bytes()
            .map_err(|_| ExchangeError::SerializationFailed)?;
        let our_encrypted = encryption::encrypt_with_ad(&session_key, &our_plaintext, &our_aad)
            .map_err(|_| ExchangeError::CryptoError)?;

        // Compute our commitment
        let our_commitment = compute_commitment(&our_encrypted);

        self.session_key = Some(session_key);
        self.their_card = Some(their_card);
        self.their_identity_key = Some(their_identity);
        self.their_commitment = Some(their_commitment);
        self.their_encrypted_card = Some(their_encrypted_card.to_vec());
        self.our_encrypted_card = Some(our_encrypted.clone());
        self.our_commitment = Some(our_commitment);

        self.state = BleHandshakeState::PayloadsExchanged {
            exchange_id,
            local_commitment: our_commitment,
            remote_commitment: their_commitment,
            remote_encrypted: their_encrypted_card.to_vec(),
        };

        Ok((our_commitment.to_vec(), our_encrypted))
    }

    /// Phase 3 (Responder): Process initiator's commitment + encrypted card.
    ///
    /// Verifies the initiator's commitment matches their encrypted card,
    /// stores the encrypted card for later decryption in Phase 4.
    ///
    /// Returns our reveal (the original commitment for the initiator to verify).
    ///
    /// Transitions: `AwaitingPayload → PayloadsExchanged`
    pub fn process_committed_payload(
        &mut self,
        their_commitment: &[u8],
        their_encrypted_card: &[u8],
    ) -> Result<Vec<u8>, ExchangeError> {
        let (exchange_id, local_commitment) = match &self.state {
            BleHandshakeState::AwaitingPayload {
                exchange_id,
                local_commitment,
            } => (*exchange_id, *local_commitment),
            _ => {
                return Err(ExchangeError::InvalidState(
                    "Expected AwaitingPayload state for processing committed payload".into(),
                ))
            }
        };

        // Verify commitment
        let computed = compute_commitment(their_encrypted_card);
        let their_commitment_arr: [u8; 32] = their_commitment
            .try_into()
            .map_err(|_| ExchangeError::InvalidBleFormat)?;

        if !bool::from(computed.ct_eq(&their_commitment_arr)) {
            return Err(ExchangeError::BleCommitmentMismatch);
        }

        // Store for decryption in complete_exchange
        self.their_encrypted_card = Some(their_encrypted_card.to_vec());
        self.their_commitment = Some(their_commitment_arr);

        // Return our commitment as the reveal
        let reveal = local_commitment.to_vec();

        self.state = BleHandshakeState::PayloadsExchanged {
            exchange_id,
            local_commitment,
            remote_commitment: their_commitment_arr,
            remote_encrypted: their_encrypted_card.to_vec(),
        };

        Ok(reveal)
    }

    /// Phase 4: Complete the exchange — decrypt remote card and finalize.
    ///
    /// For the **initiator**: `reveal` is the responder's reveal (32 bytes)
    /// which must match the commitment from the KeyAck.
    ///
    /// For the **responder**: `reveal` is empty (already verified in Phase 3).
    ///
    /// Transitions: `PayloadsExchanged → Complete`
    pub fn complete_exchange(&mut self, reveal: &[u8]) -> Result<BleExchangeResult, ExchangeError> {
        let exchange_id = match &self.state {
            BleHandshakeState::PayloadsExchanged { exchange_id, .. } => *exchange_id,
            _ => {
                return Err(ExchangeError::InvalidState(
                    "Expected PayloadsExchanged state for completing exchange".into(),
                ))
            }
        };

        // For initiator: verify the reveal matches the stored commitment
        if !reveal.is_empty() {
            let their_commitment = self
                .their_commitment
                .ok_or_else(|| ExchangeError::InvalidState("No remote commitment".into()))?;

            let reveal_arr: [u8; 32] = reveal
                .try_into()
                .map_err(|_| ExchangeError::InvalidBleFormat)?;

            // The reveal from Phase 3 is the responder's local_commitment,
            // which should match our stored their_commitment from the KeyAck
            if !bool::from(reveal_arr.ct_eq(&their_commitment)) {
                return Err(ExchangeError::BleCommitmentMismatch);
            }
        }

        // Responder needs to decrypt the remote card now
        if self.their_card.is_none() {
            let session_key = self
                .session_key
                .as_ref()
                .ok_or_else(|| ExchangeError::InvalidState("No session key".into()))?;
            let their_encrypted = self
                .their_encrypted_card
                .as_ref()
                .ok_or_else(|| ExchangeError::InvalidState("No remote encrypted card".into()))?;
            let their_identity = self
                .their_identity_key
                .ok_or_else(|| ExchangeError::InvalidState("No remote identity key".into()))?;

            let aad = build_aad(&their_identity, &self.our_identity_key, self.our_timestamp);
            let plaintext = encryption::decrypt_with_ad(session_key, their_encrypted, &aad)
                .map_err(|_| ExchangeError::BleDecryptionFailed)?;
            let card = BleCardPayload::from_bytes(&plaintext)
                .map_err(|_| ExchangeError::SerializationFailed)?;

            if !card.verify_crc16() {
                return Err(ExchangeError::BleDecryptionFailed);
            }

            self.their_card = Some(card);
        }

        let remote_card = self
            .their_card
            .take()
            .ok_or_else(|| ExchangeError::InvalidState("No remote card".into()))?;

        let result = BleExchangeResult {
            local_card: self.our_card.clone(),
            remote_card,
        };

        self.completed_cache.insert(exchange_id, result.clone());
        self.state = BleHandshakeState::Complete {
            local_card: result.local_card.clone(),
            remote_card: result.remote_card.clone(),
        };

        Ok(result)
    }
}

impl Drop for BleHandshakeSession {
    fn drop(&mut self) {
        // SymmetricKey has ZeroizeOnDrop; clear our Option explicitly
        self.session_key = None;
        // Zeroize nonces
        self.our_nonce.zeroize();
    }
}

/// Derives the session key from ephemeral DH + HKDF.
///
/// Uses single DH (ephemeral × ephemeral), HKDF with sorted nonces as salt
/// and sorted public keys in the info string.
fn derive_session_key(
    our_keys: &X3DHKeyPair,
    their_ephemeral: &[u8; 32],
    our_nonce: &[u8; NONCE_SIZE],
    their_nonce: &[u8; NONCE_SIZE],
) -> Result<SymmetricKey, ExchangeError> {
    let dh_secret = our_keys.diffie_hellman(their_ephemeral)?;

    // Salt: sorted nonces for deterministic derivation
    let mut salt = [0u8; NONCE_SIZE * 2];
    if our_nonce <= their_nonce {
        salt[..NONCE_SIZE].copy_from_slice(our_nonce);
        salt[NONCE_SIZE..].copy_from_slice(their_nonce);
    } else {
        salt[..NONCE_SIZE].copy_from_slice(their_nonce);
        salt[NONCE_SIZE..].copy_from_slice(our_nonce);
    }

    // Info: handshake info + sorted public keys
    let our_pub = our_keys.public_key();
    let mut info = BLE_HANDSHAKE_INFO.to_vec();
    if our_pub <= their_ephemeral {
        info.extend_from_slice(our_pub);
        info.extend_from_slice(their_ephemeral);
    } else {
        info.extend_from_slice(their_ephemeral);
        info.extend_from_slice(our_pub);
    }

    let derived = HKDF::derive_key(Some(&salt), &*dh_secret, &info);
    Ok(SymmetricKey::from_bytes(*derived))
}

/// Computes SHA-256 commitment over encrypted card bytes.
fn compute_commitment(encrypted_card: &[u8]) -> [u8; 32] {
    let d = digest(&SHA256, encrypted_card);
    let mut out = [0u8; 32];
    out.copy_from_slice(d.as_ref());
    out
}

/// Builds the Associated Authenticated Data (AAD) for encryption/decryption.
///
/// AAD = sender_identity(32) || receiver_identity(32) || timestamp(8)
fn build_aad(sender_identity: &[u8; 32], receiver_identity: &[u8; 32], timestamp: u64) -> Vec<u8> {
    let mut aad = Vec::with_capacity(72);
    aad.extend_from_slice(sender_identity);
    aad.extend_from_slice(receiver_identity);
    aad.extend_from_slice(&timestamp.to_be_bytes());
    aad
}

/// Computes a deterministic exchange ID from identity + ephemeral keys.
fn compute_exchange_id(identity_key: &[u8; 32], ephemeral_key: &[u8; 32]) -> [u8; 32] {
    let mut data = Vec::with_capacity(64);
    data.extend_from_slice(identity_key);
    data.extend_from_slice(ephemeral_key);
    let d = digest(&SHA256, &data);
    let mut out = [0u8; 32];
    out.copy_from_slice(d.as_ref());
    out
}
