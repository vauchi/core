//! Double Ratchet Protocol
//!
//! Implements the Double Ratchet algorithm for end-to-end encrypted messaging
//! with forward secrecy and break-in recovery. Based on the Signal Protocol.
//!
//! The Double Ratchet combines:
//! - A DH ratchet (using X25519) for break-in recovery
//! - Symmetric ratchets (chain keys) for forward secrecy

use std::collections::HashMap;
use thiserror::Error;
use zeroize::Zeroize;

use super::chain::{ChainKey, MessageKey, ChainError};
use super::encryption::{SymmetricKey, encrypt, decrypt, EncryptionError};
use super::kdf::HKDF;
use crate::exchange::X3DHKeyPair;

/// Maximum number of skipped message keys to store.
const MAX_SKIPPED_KEYS: usize = 1000;

/// Double Ratchet error types.
#[derive(Error, Debug)]
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
}

/// KDF info constants for domain separation.
const ROOT_RATCHET_INFO: &[u8] = b"WebBook_Root_Ratchet";

/// A ratcheted message ready for transmission.
#[derive(Debug, Clone)]
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
    ) -> Self {
        // Generate our first DH keypair
        let our_dh = X3DHKeyPair::generate();

        // Perform initial DH to get first root key and send chain
        let dh_output = our_dh.diffie_hellman(&their_dh_public);
        let (root_key, send_chain_key) = HKDF::derive_key_pair(
            Some(x3dh_secret.as_bytes()),
            &dh_output,
            ROOT_RATCHET_INFO,
        );

        DoubleRatchetState {
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
        }
    }

    /// Initialize as the responder (Bob) after X3DH.
    ///
    /// The responder waits for Alice's first message before they can send.
    pub fn initialize_responder(
        x3dh_secret: &SymmetricKey,
        our_dh: X3DHKeyPair,
    ) -> Self {
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
        // Ensure we have a sending chain
        let send_chain = self.send_chain.as_ref()
            .ok_or_else(|| RatchetError::InvalidMessage(
                "Cannot send: no sending chain (responder must receive first)".into()
            ))?;

        // Ratchet the chain to get message key
        let (message_key, next_chain) = send_chain.ratchet()?;
        self.send_chain = Some(next_chain);

        // Encrypt the plaintext
        let ciphertext = encrypt(message_key.symmetric_key(), plaintext)?;

        let message = RatchetMessage {
            dh_public: self.our_public_key(),
            dh_generation: self.dh_generation,
            message_index: self.send_message_count,
            previous_chain_length: self.previous_send_chain_length,
            ciphertext,
        };

        self.send_message_count += 1;

        Ok(message)
    }

    /// Decrypt a received message using the Double Ratchet.
    ///
    /// Handles DH ratchet steps and out-of-order messages.
    pub fn decrypt(&mut self, message: &RatchetMessage) -> Result<Vec<u8>, RatchetError> {
        // Try skipped keys first
        if let Some(key) = self.try_skipped_key(message) {
            return decrypt(key.symmetric_key(), &message.ciphertext)
                .map_err(RatchetError::from);
        }

        // Check if we need to perform a DH ratchet
        let their_dh_changed = self.their_dh
            .map(|k| k != message.dh_public)
            .unwrap_or(true);

        if their_dh_changed {
            // Skip any remaining messages in current receiving chain (previous DH generation)
            if self.recv_chain.is_some() {
                let prev_gen = if self.dh_generation > 0 { self.dh_generation - 1 } else { 0 };
                self.skip_messages_for_gen(message.previous_chain_length, prev_gen)?;
            }

            // Perform DH ratchet
            self.dh_ratchet(&message.dh_public)?;
        }

        // Skip messages in current chain if needed (using message's generation)
        self.skip_messages_for_gen(message.message_index, message.dh_generation)?;

        // Get the message key
        let recv_chain = self.recv_chain.as_ref()
            .ok_or_else(|| RatchetError::InvalidMessage("No receiving chain".into()))?;

        let (message_key, next_chain) = recv_chain.ratchet()?;
        self.recv_chain = Some(next_chain);
        self.recv_message_count = message.message_index + 1;

        // Decrypt
        decrypt(message_key.symmetric_key(), &message.ciphertext)
            .map_err(RatchetError::from)
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

        // Skip forward and store keys
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
        let dh_recv = self.our_dh.diffie_hellman(their_new_public);
        let (root_key, recv_chain_key) = HKDF::derive_key_pair(
            Some(&self.root_key),
            &dh_recv,
            ROOT_RATCHET_INFO,
        );
        self.root_key = root_key;
        self.recv_chain = Some(ChainKey::new(recv_chain_key));
        self.recv_message_count = 0;

        // Generate new DH keypair
        self.previous_send_chain_length = self.send_message_count;
        self.our_dh = X3DHKeyPair::generate();

        // DH with their key and our NEW key -> new sending chain
        let dh_send = self.our_dh.diffie_hellman(their_new_public);
        let (root_key, send_chain_key) = HKDF::derive_key_pair(
            Some(&self.root_key),
            &dh_send,
            ROOT_RATCHET_INFO,
        );
        self.root_key = root_key;
        self.send_chain = Some(ChainKey::new(send_chain_key));
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_pair() -> (DoubleRatchetState, DoubleRatchetState) {
        // Simulate X3DH: both parties derive the same shared secret
        let shared_secret = SymmetricKey::from_bytes([42u8; 32]);

        // Bob's initial DH keypair (used in X3DH)
        let bob_dh = X3DHKeyPair::generate();
        let bob_public = *bob_dh.public_key();

        // Alice initializes as initiator with Bob's public key
        let alice = DoubleRatchetState::initialize_initiator(&shared_secret, bob_public);

        // Bob initializes as responder with his keypair
        let bob = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

        (alice, bob)
    }

    #[test]
    fn test_dr_encrypt_decrypt_roundtrip() {
        let (mut alice, mut bob) = create_test_pair();

        // Alice sends to Bob
        let plaintext = b"Hello Bob!";
        let message = alice.encrypt(plaintext).unwrap();
        let decrypted = bob.decrypt(&message).unwrap();

        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn test_dr_bidirectional_communication() {
        let (mut alice, mut bob) = create_test_pair();

        // Alice -> Bob
        let msg1 = alice.encrypt(b"Hello Bob").unwrap();
        let dec1 = bob.decrypt(&msg1).unwrap();
        assert_eq!(b"Hello Bob".as_slice(), dec1.as_slice());

        // Bob -> Alice
        let msg2 = bob.encrypt(b"Hello Alice").unwrap();
        let dec2 = alice.decrypt(&msg2).unwrap();
        assert_eq!(b"Hello Alice".as_slice(), dec2.as_slice());

        // Alice -> Bob again
        let msg3 = alice.encrypt(b"How are you?").unwrap();
        let dec3 = bob.decrypt(&msg3).unwrap();
        assert_eq!(b"How are you?".as_slice(), dec3.as_slice());
    }

    #[test]
    fn test_dr_forward_secrecy() {
        let (mut alice, mut bob) = create_test_pair();

        // Alice sends multiple messages
        let msg1 = alice.encrypt(b"Message 1").unwrap();
        let msg2 = alice.encrypt(b"Message 2").unwrap();

        // Bob decrypts message 1
        bob.decrypt(&msg1).unwrap();

        // Even if we had access to current keys, we can't decrypt msg1 again
        // (the key was consumed)
        // This is forward secrecy - old keys are deleted

        // But msg2 still works
        let dec2 = bob.decrypt(&msg2).unwrap();
        assert_eq!(b"Message 2".as_slice(), dec2.as_slice());
    }

    #[test]
    fn test_dr_out_of_order_messages() {
        let (mut alice, mut bob) = create_test_pair();

        // Alice sends three messages
        let msg1 = alice.encrypt(b"First").unwrap();
        let msg2 = alice.encrypt(b"Second").unwrap();
        let msg3 = alice.encrypt(b"Third").unwrap();

        // Bob receives them out of order
        let dec3 = bob.decrypt(&msg3).unwrap();
        assert_eq!(b"Third".as_slice(), dec3.as_slice());

        let dec1 = bob.decrypt(&msg1).unwrap();
        assert_eq!(b"First".as_slice(), dec1.as_slice());

        let dec2 = bob.decrypt(&msg2).unwrap();
        assert_eq!(b"Second".as_slice(), dec2.as_slice());
    }

    #[test]
    fn test_dr_dh_ratchet_on_reply() {
        let (mut alice, mut bob) = create_test_pair();

        let initial_alice_dh = alice.our_public_key();

        // Alice sends
        let msg1 = alice.encrypt(b"Hello").unwrap();
        bob.decrypt(&msg1).unwrap();

        // Bob replies - this triggers DH ratchet for Bob
        let msg2 = bob.encrypt(b"Hi").unwrap();
        alice.decrypt(&msg2).unwrap();

        // Alice's DH key changes when she sends again
        let _msg3 = alice.encrypt(b"Bye").unwrap();

        // Alice's DH key should have changed
        assert_ne!(initial_alice_dh, alice.our_public_key());
    }

    #[test]
    fn test_dr_multiple_ratchets() {
        let (mut alice, mut bob) = create_test_pair();

        // Multiple back-and-forth exchanges
        for i in 0..5 {
            let msg_a = alice.encrypt(format!("Alice {}", i).as_bytes()).unwrap();
            bob.decrypt(&msg_a).unwrap();

            let msg_b = bob.encrypt(format!("Bob {}", i).as_bytes()).unwrap();
            alice.decrypt(&msg_b).unwrap();
        }

        // Both should have ratcheted multiple times
        assert!(alice.dh_generation() > 0);
        assert!(bob.dh_generation() > 0);
    }

    #[test]
    fn test_dr_responder_cannot_send_first() {
        let shared_secret = SymmetricKey::from_bytes([42u8; 32]);
        let bob_dh = X3DHKeyPair::generate();
        let mut bob = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

        // Bob (responder) tries to send first - should fail
        let result = bob.encrypt(b"Hello");
        assert!(result.is_err());
    }

    #[test]
    fn test_dr_different_keys_per_message() {
        let (mut alice, _bob) = create_test_pair();

        let msg1 = alice.encrypt(b"Test 1").unwrap();
        let msg2 = alice.encrypt(b"Test 2").unwrap();

        // Ciphertexts should be different (different keys used)
        assert_ne!(msg1.ciphertext, msg2.ciphertext);

        // Message indices should increment
        assert_eq!(msg1.message_index, 0);
        assert_eq!(msg2.message_index, 1);
    }

    #[test]
    fn test_dr_skipped_message_limit() {
        let (mut alice, mut bob) = create_test_pair();

        // Send many messages
        let mut messages = Vec::new();
        for i in 0..100 {
            messages.push(alice.encrypt(format!("Msg {}", i).as_bytes()).unwrap());
        }

        // Skip to message 99 first
        bob.decrypt(&messages[99]).unwrap();

        // This should have stored 99 skipped keys
        assert_eq!(bob.skipped_keys_count(), 99);

        // Now we can decrypt the skipped messages
        for i in 0..99 {
            let dec = bob.decrypt(&messages[i]).unwrap();
            assert_eq!(format!("Msg {}", i).as_bytes(), dec.as_slice());
        }

        // Skipped keys should be consumed
        assert_eq!(bob.skipped_keys_count(), 0);
    }

    #[test]
    fn test_dr_empty_message() {
        let (mut alice, mut bob) = create_test_pair();

        let msg = alice.encrypt(b"").unwrap();
        let dec = bob.decrypt(&msg).unwrap();

        assert!(dec.is_empty());
    }

    #[test]
    fn test_dr_large_message() {
        let (mut alice, mut bob) = create_test_pair();

        let large_data = vec![0xABu8; 100_000];
        let msg = alice.encrypt(&large_data).unwrap();
        let dec = bob.decrypt(&msg).unwrap();

        assert_eq!(large_data, dec);
    }
}
