// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Double Ratchet Protocol
//!
//! Implements the Double Ratchet algorithm for end-to-end encrypted messaging
//! with forward secrecy and break-in recovery. Based on the Signal Protocol.
//!
//! The Double Ratchet combines:
//! - A DH ratchet (using X25519) for break-in recovery
//! - Symmetric ratchets (chain keys) for forward secrecy

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use subtle::ConstantTimeEq;
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::X3DHKeyPair;
use super::chain::{ChainError, ChainKey, MessageKey};
use super::encryption::{EncryptionError, SymmetricKey, decrypt_with_ad, encrypt_with_ad};
use super::kdf::HKDF;
use super::padding;

/// Maximum number of skipped message keys to store.
const MAX_SKIPPED_KEYS: usize = 1000;

/// Double Ratchet error types.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum RatchetError {
    #[error("Chain error: {0}")]
    Chain(#[from] ChainError),

    #[error("Encryption error: {0}")]
    Encryption(#[from] EncryptionError),

    #[error("Too many skipped messages")]
    TooManySkipped,

    #[error("Duplicate message (already decrypted)")]
    DuplicateMessage,

    #[error("Invalid message: {0}")]
    InvalidMessage(String),

    /// The responder has no sending chain yet — it must decrypt the initiator's
    /// first message before it can send. Non-cryptographic + transient: callers
    /// should DEFER the send (keep it owed and retry after the first inbound
    /// message), not treat it as a hard failure. Seeding a responder send chain
    /// at init would be a crypto bug (no matching initiator receive chain).
    #[error(
        "Cannot send: responder must receive the initiator's first message before it has a sending chain"
    )]
    NoSendingChain,

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("DH validation failed: {0}")]
    DhValidation(#[from] super::DhError),
}

/// Current serialization format version for ratchet state (#236).
///
/// Increment when changing the `SerializedRatchetState` layout.
/// Deserialization checks this version and rejects unknown future versions.
pub const RATCHET_STATE_VERSION: u8 = 1;

/// Serializable representation of DoubleRatchetState.
///
/// This struct captures all necessary state to persist and restore a ratchet session.
/// Contains sensitive cryptographic material that is zeroized on drop.
///
/// The `version` field enables schema evolution: new fields can be added in future
/// versions while maintaining backward compatibility for loading older states.
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct SerializedRatchetState {
    /// Schema version for forward compatibility (#236)
    #[serde(default = "default_ratchet_version")]
    pub version: u8,
    /// Root key for deriving new chain keys
    pub root_key: [u8; 32],
    /// Our DH secret key bytes
    pub our_dh_secret: [u8; 32],
    /// Their current DH public key
    pub their_dh: Option<[u8; 32]>,
    /// Sending chain key and generation
    pub send_chain: Option<([u8; 32], u32)>,
    /// Receiving chain key and generation
    pub recv_chain: Option<([u8; 32], u32)>,
    /// Current DH ratchet generation
    pub dh_generation: u32,
    /// Number of messages sent in current sending chain
    pub send_message_count: u32,
    /// Number of messages received in current receiving chain
    pub recv_message_count: u32,
    /// Previous sending chain length
    pub previous_send_chain_length: u32,
    /// Skipped message keys: (dh_gen, msg_index) -> key_bytes
    pub skipped_keys: Vec<((u32, u32), [u8; 32])>,
}

/// Default version for deserializing ratchet states that predate the version field.
fn default_ratchet_version() -> u8 {
    1
}

impl std::fmt::Debug for SerializedRatchetState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SerializedRatchetState")
            .field("version", &self.version)
            .field("dh_generation", &self.dh_generation)
            .field("send_message_count", &self.send_message_count)
            .field("recv_message_count", &self.recv_message_count)
            .field("skipped_keys_count", &self.skipped_keys.len())
            .finish_non_exhaustive()
    }
}

/// KDF info constants for domain separation.
const ROOT_RATCHET_INFO: &[u8] = b"Vauchi_Root_Ratchet";

/// A ratcheted message ready for transmission.
///
/// ## Header encryption (#229 trade-off)
///
/// The Signal spec describes optional header encryption to hide `dh_public`,
/// `dh_generation`, and `message_index` from observers. Vauchi does **not**
/// implement header encryption because:
///
/// - All traffic already flows over TLS to the relay — headers are not
///   visible to passive network observers.
/// - The relay is untrusted but already sees opaque WebSocket frames; header
///   metadata does not meaningfully increase its knowledge beyond message
///   count and timing (which header encryption cannot hide).
/// - Header encryption adds complexity (header keys, sealed sender chains)
///   for marginal benefit in our relay-mediated topology.
///
/// If the threat model changes (e.g., direct peer-to-peer transport over
/// unencrypted channels), header encryption should be reconsidered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatchetMessage {
    /// Sender's current DH public key
    pub dh_public: [u8; 32],
    /// Which DH ratchet step this message is from
    pub dh_generation: u32,
    /// Message number within this chain
    pub message_index: u32,
    /// Previous chain length (for detecting skipped messages)
    pub previous_chain_length: u32,
    /// The encrypted payload
    pub ciphertext: Vec<u8>,
}

impl RatchetMessage {
    /// Constructs associated data from the message header for AEAD binding.
    ///
    /// AD = dh_public(32) || dh_generation(4 BE) || message_index(4 BE) || previous_chain_length(4 BE)
    ///
    /// Binding the header into the AEAD prevents header manipulation attacks
    /// (e.g., changing message ordering or swapping sender DH keys).
    fn associated_data(&self) -> [u8; 44] {
        let mut ad = [0u8; 44];
        ad[0..32].copy_from_slice(&self.dh_public);
        ad[32..36].copy_from_slice(&self.dh_generation.to_be_bytes());
        ad[36..40].copy_from_slice(&self.message_index.to_be_bytes());
        ad[40..44].copy_from_slice(&self.previous_chain_length.to_be_bytes());
        ad
    }
}

/// The Double Ratchet state machine.
///
/// Maintains the cryptographic state for secure bidirectional communication
/// with a single contact.
pub struct DoubleRatchetState {
    /// Root key for deriving new chain keys
    root_key: [u8; 32],
    /// Our current DH keypair
    our_dh: X3DHKeyPair,
    /// Their current DH public key (None until we receive a message)
    their_dh: Option<[u8; 32]>,
    /// Sending chain key
    send_chain: Option<ChainKey>,
    /// Receiving chain key
    recv_chain: Option<ChainKey>,
    /// Current DH ratchet generation (increments on each DH ratchet)
    dh_generation: u32,
    /// Number of messages sent in current sending chain
    send_message_count: u32,
    /// Number of messages received in current receiving chain
    recv_message_count: u32,
    /// Previous sending chain length (for message header)
    previous_send_chain_length: u32,
    /// Stored skipped message keys: (dh_gen, msg_index) -> MessageKey
    skipped_keys: HashMap<(u32, u32), MessageKey>,
}

impl std::fmt::Debug for DoubleRatchetState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DoubleRatchetState")
            .field("dh_generation", &self.dh_generation)
            .field("send_message_count", &self.send_message_count)
            .field("recv_message_count", &self.recv_message_count)
            .field("skipped_keys_count", &self.skipped_keys.len())
            .finish()
    }
}

impl Drop for DoubleRatchetState {
    fn drop(&mut self) {
        self.root_key.zeroize();
        self.skipped_keys.clear();
    }
}

impl DoubleRatchetState {
    /// Initialize as the initiator (Alice) after X3DH.
    ///
    /// The initiator has performed X3DH and knows Bob's public key.
    /// They will send the first message.
    pub fn initialize_initiator(
        x3dh_secret: &SymmetricKey,
        their_dh_public: [u8; 32],
    ) -> Result<Self, RatchetError> {
        let our_dh = X3DHKeyPair::generate();

        let dh_output = our_dh.diffie_hellman(&their_dh_public)?;
        let (root_key_z, send_chain_key_z) =
            HKDF::derive_key_pair(Some(x3dh_secret.as_bytes()), &*dh_output, ROOT_RATCHET_INFO);
        let root_key = *root_key_z;
        let send_chain_key = *send_chain_key_z;

        Ok(DoubleRatchetState {
            root_key,
            our_dh,
            their_dh: Some(their_dh_public),
            send_chain: Some(ChainKey::new(send_chain_key)),
            recv_chain: None,
            dh_generation: 0,
            send_message_count: 0,
            recv_message_count: 0,
            previous_send_chain_length: 0,
            skipped_keys: HashMap::new(),
        })
    }

    /// Initialize as the responder (Bob) after X3DH.
    ///
    /// The responder waits for Alice's first message before they can send.
    pub fn initialize_responder(x3dh_secret: &SymmetricKey, our_dh: X3DHKeyPair) -> Self {
        DoubleRatchetState {
            root_key: *x3dh_secret.as_bytes(),
            our_dh,
            their_dh: None,
            send_chain: None,
            recv_chain: None,
            dh_generation: 0,
            send_message_count: 0,
            recv_message_count: 0,
            previous_send_chain_length: 0,
            skipped_keys: HashMap::new(),
        }
    }

    /// Returns our current DH public key.
    pub fn our_public_key(&self) -> [u8; 32] {
        *self.our_dh.public_key()
    }

    /// Encrypt a message using the Double Ratchet.
    ///
    /// Advances the sending chain and returns an encrypted message.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<RatchetMessage, RatchetError> {
        let send_chain = self
            .send_chain
            .as_ref()
            .ok_or(RatchetError::NoSendingChain)?;

        let (message_key, next_chain) = send_chain.ratchet()?;
        self.send_chain = Some(next_chain);

        // Pad plaintext to fixed bucket size to prevent traffic analysis
        let padded = padding::pad(plaintext);

        // Construct header before encryption so we can bind it as AEAD associated data
        let header = RatchetMessage {
            dh_public: self.our_public_key(),
            dh_generation: self.dh_generation,
            message_index: self.send_message_count,
            previous_chain_length: self.previous_send_chain_length,
            ciphertext: Vec::new(),
        };
        let ad = header.associated_data();
        let ciphertext = encrypt_with_ad(message_key.symmetric_key(), &padded, &ad)?;

        let message = RatchetMessage {
            ciphertext,
            ..header
        };

        self.send_message_count += 1;

        Ok(message)
    }

    /// Decrypt a received message using the Double Ratchet.
    ///
    /// Handles DH ratchet steps and out-of-order messages. State advances only
    /// after the ciphertext authenticates and its padding validates.
    pub fn decrypt(&mut self, message: &RatchetMessage) -> Result<Vec<u8>, RatchetError> {
        let mut candidate = Self::deserialize(self.serialize())?;
        let plaintext = candidate.decrypt_in_place(message)?;
        *self = candidate;
        Ok(plaintext)
    }

    /// Performs decryption on a disposable candidate state.
    fn decrypt_in_place(&mut self, message: &RatchetMessage) -> Result<Vec<u8>, RatchetError> {
        let ad = message.associated_data();

        if let Some(key) = self.try_skipped_key(message) {
            let padded = decrypt_with_ad(key.symmetric_key(), &message.ciphertext, &ad)
                .map_err(RatchetError::from)?;
            return padding::unpad(&padded).ok_or_else(|| {
                RatchetError::InvalidMessage("Failed to unpad decrypted message".into())
            });
        }

        let their_dh_changed = self
            .their_dh
            .map(|k| !bool::from(k.ct_eq(&message.dh_public)))
            .unwrap_or(true);

        if their_dh_changed {
            // Skip any remaining messages in current receiving chain (previous DH generation)
            if self.recv_chain.is_some() {
                let prev_gen = if self.dh_generation > 0 {
                    self.dh_generation - 1
                } else {
                    0
                };
                self.skip_messages_for_gen(message.previous_chain_length, prev_gen)?;
            }

            self.dh_ratchet(&message.dh_public)?;
        }

        self.skip_messages_for_gen(message.message_index, message.dh_generation)?;

        let recv_chain = self
            .recv_chain
            .as_ref()
            .ok_or_else(|| RatchetError::InvalidMessage("No receiving chain".into()))?;

        let (message_key, next_chain) = recv_chain.ratchet()?;
        self.recv_chain = Some(next_chain);
        self.recv_message_count = message.message_index + 1;

        let padded = decrypt_with_ad(message_key.symmetric_key(), &message.ciphertext, &ad)
            .map_err(RatchetError::from)?;
        padding::unpad(&padded)
            .ok_or_else(|| RatchetError::InvalidMessage("Failed to unpad decrypted message".into()))
    }

    /// Try to decrypt using a previously skipped key.
    fn try_skipped_key(&mut self, message: &RatchetMessage) -> Option<MessageKey> {
        let key = (message.dh_generation, message.message_index);
        self.skipped_keys.remove(&key)
    }

    /// Skip messages and store their keys for later decryption.
    ///
    /// The `dh_gen` parameter specifies which DH generation these skipped keys belong to.
    fn skip_messages_for_gen(&mut self, until: u32, dh_gen: u32) -> Result<(), RatchetError> {
        let recv_chain = match &self.recv_chain {
            Some(chain) => chain,
            None => return Ok(()), // No chain yet, nothing to skip
        };

        let current = recv_chain.generation();
        if until <= current {
            return Ok(()); // Nothing to skip
        }

        let skip_count = (until - current) as usize;
        if self.skipped_keys.len() + skip_count > MAX_SKIPPED_KEYS {
            return Err(RatchetError::TooManySkipped);
        }

        let (skipped, new_chain) = recv_chain.skip_to(until)?;
        self.recv_chain = Some(new_chain);

        for (i, key) in skipped.into_iter().enumerate() {
            let msg_index = current + i as u32;
            self.skipped_keys.insert((dh_gen, msg_index), key);
        }

        Ok(())
    }

    /// Perform a DH ratchet step.
    fn dh_ratchet(&mut self, their_new_public: &[u8; 32]) -> Result<(), RatchetError> {
        self.their_dh = Some(*their_new_public);

        // DH with their new key and our current key -> new receiving chain
        let dh_recv = self.our_dh.diffie_hellman(their_new_public)?;
        let (root_key_z, recv_chain_key_z) =
            HKDF::derive_key_pair(Some(&self.root_key), &*dh_recv, ROOT_RATCHET_INFO);
        self.root_key = *root_key_z;
        self.recv_chain = Some(ChainKey::new(*recv_chain_key_z));
        self.recv_message_count = 0;

        self.previous_send_chain_length = self.send_message_count;
        self.our_dh = X3DHKeyPair::generate();

        // DH with their key and our NEW key -> new sending chain
        let dh_send = self.our_dh.diffie_hellman(their_new_public)?;
        let (root_key_z, send_chain_key_z) =
            HKDF::derive_key_pair(Some(&self.root_key), &*dh_send, ROOT_RATCHET_INFO);
        self.root_key = *root_key_z;
        self.send_chain = Some(ChainKey::new(*send_chain_key_z));
        self.send_message_count = 0;

        self.dh_generation += 1;

        Ok(())
    }

    /// Returns the number of skipped keys currently stored.
    pub fn skipped_keys_count(&self) -> usize {
        self.skipped_keys.len()
    }

    /// Returns the current DH generation.
    pub fn dh_generation(&self) -> u32 {
        self.dh_generation
    }

    /// Serializes the ratchet state for persistence.
    pub fn serialize(&self) -> SerializedRatchetState {
        SerializedRatchetState {
            version: RATCHET_STATE_VERSION,
            root_key: self.root_key,
            our_dh_secret: *self.our_dh.secret_bytes(),
            their_dh: self.their_dh,
            send_chain: self
                .send_chain
                .as_ref()
                .map(|c| (*c.as_bytes(), c.generation())),
            recv_chain: self
                .recv_chain
                .as_ref()
                .map(|c| (*c.as_bytes(), c.generation())),
            dh_generation: self.dh_generation,
            send_message_count: self.send_message_count,
            recv_message_count: self.recv_message_count,
            previous_send_chain_length: self.previous_send_chain_length,
            skipped_keys: self
                .skipped_keys
                .iter()
                .map(|(k, v)| (*k, *v.symmetric_key().as_bytes()))
                .collect(),
        }
    }

    /// Deserializes a ratchet state from its serialized form.
    ///
    /// Validates the version field and rejects unknown future versions (#236).
    pub fn deserialize(mut s: SerializedRatchetState) -> Result<Self, RatchetError> {
        if s.version > RATCHET_STATE_VERSION {
            return Err(RatchetError::Deserialization(format!(
                "unsupported ratchet state version {} (max supported: {})",
                s.version, RATCHET_STATE_VERSION
            )));
        }
        // Use std::mem::take to extract values, leaving zeros behind for Drop
        let root_key = std::mem::take(&mut s.root_key);
        let our_dh_secret = Zeroizing::new(std::mem::take(&mut s.our_dh_secret));
        let their_dh = s.their_dh.take();
        let send_chain_data = s.send_chain.take();
        let recv_chain_data = s.recv_chain.take();
        let skipped_keys_data = std::mem::take(&mut s.skipped_keys);

        let our_dh = X3DHKeyPair::from_bytes(*our_dh_secret);

        let send_chain = send_chain_data.map(|(key, r#gen)| ChainKey::with_generation(key, r#gen));
        let recv_chain = recv_chain_data.map(|(key, r#gen)| ChainKey::with_generation(key, r#gen));

        let skipped_keys = skipped_keys_data
            .into_iter()
            .map(|(k, v)| (k, MessageKey::from_bytes(v)))
            .collect();

        Ok(DoubleRatchetState {
            root_key,
            our_dh,
            their_dh,
            send_chain,
            recv_chain,
            dh_generation: s.dh_generation,
            send_message_count: s.send_message_count,
            recv_message_count: s.recv_message_count,
            previous_send_chain_length: s.previous_send_chain_length,
            skipped_keys,
        })
    }
}
