// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Chain Key Ratcheting
//!
//! Implements symmetric key ratcheting for the Double Ratchet protocol.
//! Each chain key can derive a message key and advance to the next chain key.

use super::encryption::SymmetricKey;
use super::kdf::HKDF;
use thiserror::Error;
use zeroize::Zeroize;

/// Maximum number of chain generations to prevent abuse.
const MAX_CHAIN_GENERATIONS: u32 = 2000;

/// Maximum number of keys that can be skipped.
const MAX_SKIP: u32 = 1000;

/// Chain key ratcheting error types.
#[derive(Error, Debug)]
pub enum ChainError {
    #[error("Chain generation limit exceeded (max {MAX_CHAIN_GENERATIONS})")]
    GenerationLimitExceeded,

    #[error("Skip limit exceeded (max {MAX_SKIP} keys)")]
    SkipLimitExceeded,

    #[error("Cannot skip backwards (current: {current}, target: {target})")]
    CannotSkipBackwards { current: u32, target: u32 },
}

/// KDF info constants for domain separation.
const CHAIN_KEY_INFO: &[u8] = b"Vauchi_Chain_Key";
const MESSAGE_KEY_INFO: &[u8] = b"Vauchi_Message_Key";

/// A chain key used for symmetric ratcheting.
///
/// Chain keys are never used directly for encryption. They derive:
/// - Message keys (for actual encryption)
/// - The next chain key (for ratcheting forward)
#[derive(Clone)]
pub struct ChainKey {
    key: [u8; 32],
    generation: u32,
}

impl std::fmt::Debug for ChainKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChainKey")
            .field("key", &"[REDACTED]")
            .field("generation", &self.generation)
            .finish()
    }
}

impl Drop for ChainKey {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

impl ChainKey {
    /// Creates a new chain key from raw bytes at generation 0.
    pub fn new(key: [u8; 32]) -> Self {
        ChainKey { key, generation: 0 }
    }

    /// Creates a chain key at a specific generation.
    pub fn with_generation(key: [u8; 32], generation: u32) -> Self {
        ChainKey { key, generation }
    }

    /// Returns the current generation number.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Advances the chain by one step, returning a message key and the next chain key.
    ///
    /// This implements the symmetric ratchet step:
    /// - message_key = KDF(chain_key, MESSAGE_KEY_INFO)
    /// - next_chain_key = KDF(chain_key, CHAIN_KEY_INFO)
    pub fn ratchet(&self) -> Result<(MessageKey, ChainKey), ChainError> {
        if self.generation >= MAX_CHAIN_GENERATIONS {
            return Err(ChainError::GenerationLimitExceeded);
        }

        // Derive message key
        let message_key_bytes = HKDF::derive_key(None, &self.key, MESSAGE_KEY_INFO);

        // Derive next chain key
        let next_chain_key_bytes = HKDF::derive_key(None, &self.key, CHAIN_KEY_INFO);

        let message_key = MessageKey {
            key: SymmetricKey::from_bytes(message_key_bytes),
            generation: self.generation,
        };

        let next_chain = ChainKey {
            key: next_chain_key_bytes,
            generation: self.generation + 1,
        };

        Ok((message_key, next_chain))
    }

    /// Skips forward to a target generation, returning all intermediate message keys.
    ///
    /// Used when receiving out-of-order messages to derive keys for skipped messages.
    pub fn skip_to(&self, target: u32) -> Result<(Vec<MessageKey>, ChainKey), ChainError> {
        if target < self.generation {
            return Err(ChainError::CannotSkipBackwards {
                current: self.generation,
                target,
            });
        }

        let skip_count = target - self.generation;
        if skip_count > MAX_SKIP {
            return Err(ChainError::SkipLimitExceeded);
        }

        let mut keys = Vec::with_capacity(skip_count as usize);
        let mut current = self.clone();

        while current.generation < target {
            let (msg_key, next) = current.ratchet()?;
            keys.push(msg_key);
            current = next;
        }

        Ok((keys, current))
    }

    /// Returns a reference to the raw key bytes (for serialization).
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.key
    }
}

/// A message encryption key derived from a chain key.
///
/// Message keys are single-use and should be deleted after use
/// to provide forward secrecy.
pub struct MessageKey {
    key: SymmetricKey,
    generation: u32,
}

impl std::fmt::Debug for MessageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageKey")
            .field("key", &"[REDACTED]")
            .field("generation", &self.generation)
            .finish()
    }
}

impl MessageKey {
    /// Returns the generation this key was derived at.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Returns the underlying symmetric key for encryption.
    pub fn symmetric_key(&self) -> &SymmetricKey {
        &self.key
    }

    /// Consumes self and returns the underlying symmetric key.
    pub fn into_symmetric_key(self) -> SymmetricKey {
        self.key
    }

    /// Creates a MessageKey from raw bytes (for deserialization).
    ///
    /// Note: Generation is set to 0 since skipped keys don't track their generation
    /// after being stored.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        MessageKey {
            key: SymmetricKey::from_bytes(bytes),
            generation: 0,
        }
    }
}

// INLINE_TEST_REQUIRED: Tests private MAX_SKIP and MAX_CHAIN_GENERATIONS constants for boundary conditions
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_key_ratchet_produces_unique_keys() {
        let initial = ChainKey::new([0u8; 32]);

        let (msg1, chain1) = initial.ratchet().unwrap();
        let (msg2, chain2) = chain1.ratchet().unwrap();
        let (msg3, _chain3) = chain2.ratchet().unwrap();

        // All message keys should be different
        assert_ne!(
            msg1.symmetric_key().as_bytes(),
            msg2.symmetric_key().as_bytes()
        );
        assert_ne!(
            msg2.symmetric_key().as_bytes(),
            msg3.symmetric_key().as_bytes()
        );
        assert_ne!(
            msg1.symmetric_key().as_bytes(),
            msg3.symmetric_key().as_bytes()
        );
    }

    #[test]
    fn test_chain_key_deterministic() {
        let key_bytes = [42u8; 32];

        let chain1 = ChainKey::new(key_bytes);
        let chain2 = ChainKey::new(key_bytes);

        let (msg1, next1) = chain1.ratchet().unwrap();
        let (msg2, next2) = chain2.ratchet().unwrap();

        // Same input should produce same output
        assert_eq!(
            msg1.symmetric_key().as_bytes(),
            msg2.symmetric_key().as_bytes()
        );
        assert_eq!(next1.as_bytes(), next2.as_bytes());
    }

    #[test]
    fn test_chain_key_generation_increments() {
        let chain = ChainKey::new([0u8; 32]);
        assert_eq!(chain.generation(), 0);

        let (_msg, chain) = chain.ratchet().unwrap();
        assert_eq!(chain.generation(), 1);

        let (_msg, chain) = chain.ratchet().unwrap();
        assert_eq!(chain.generation(), 2);
    }

    #[test]
    fn test_chain_key_skip_forward() {
        let chain = ChainKey::new([0u8; 32]);

        // Skip to generation 5
        let (skipped_keys, final_chain) = chain.skip_to(5).unwrap();

        assert_eq!(skipped_keys.len(), 5);
        assert_eq!(final_chain.generation(), 5);

        // Verify generations of skipped keys
        for (i, key) in skipped_keys.iter().enumerate() {
            assert_eq!(key.generation(), i as u32);
        }
    }

    #[test]
    fn test_chain_key_skip_produces_same_keys_as_sequential() {
        let chain1 = ChainKey::new([99u8; 32]);
        let chain2 = ChainKey::new([99u8; 32]);

        // Skip forward
        let (skipped, _) = chain1.skip_to(3).unwrap();

        // Sequential ratchet
        let (msg0, chain2) = chain2.ratchet().unwrap();
        let (msg1, chain2) = chain2.ratchet().unwrap();
        let (msg2, _) = chain2.ratchet().unwrap();

        // Should produce same keys
        assert_eq!(
            skipped[0].symmetric_key().as_bytes(),
            msg0.symmetric_key().as_bytes()
        );
        assert_eq!(
            skipped[1].symmetric_key().as_bytes(),
            msg1.symmetric_key().as_bytes()
        );
        assert_eq!(
            skipped[2].symmetric_key().as_bytes(),
            msg2.symmetric_key().as_bytes()
        );
    }

    #[test]
    fn test_chain_key_cannot_skip_backwards() {
        let chain = ChainKey::with_generation([0u8; 32], 10);

        let result = chain.skip_to(5);
        assert!(matches!(
            result,
            Err(ChainError::CannotSkipBackwards {
                current: 10,
                target: 5
            })
        ));
    }

    #[test]
    fn test_chain_key_skip_limit() {
        let chain = ChainKey::new([0u8; 32]);

        // Skipping MAX_SKIP + 1 should fail
        let result = chain.skip_to(MAX_SKIP + 1);
        assert!(matches!(result, Err(ChainError::SkipLimitExceeded)));

        // Skipping exactly MAX_SKIP should succeed
        let result = chain.skip_to(MAX_SKIP);
        assert!(result.is_ok());
    }

    #[test]
    fn test_chain_key_max_generation_limit() {
        let chain = ChainKey::with_generation([0u8; 32], MAX_CHAIN_GENERATIONS);

        let result = chain.ratchet();
        assert!(matches!(result, Err(ChainError::GenerationLimitExceeded)));
    }

    #[test]
    fn test_message_key_generation() {
        let chain = ChainKey::with_generation([0u8; 32], 42);
        let (msg_key, _) = chain.ratchet().unwrap();

        assert_eq!(msg_key.generation(), 42);
    }

    #[test]
    fn test_different_initial_keys_different_outputs() {
        let chain1 = ChainKey::new([1u8; 32]);
        let chain2 = ChainKey::new([2u8; 32]);

        let (msg1, _) = chain1.ratchet().unwrap();
        let (msg2, _) = chain2.ratchet().unwrap();

        assert_ne!(
            msg1.symmetric_key().as_bytes(),
            msg2.symmetric_key().as_bytes()
        );
    }

    #[test]
    fn test_skip_to_same_generation_is_noop() {
        let chain = ChainKey::with_generation([0u8; 32], 5);

        let (skipped, final_chain) = chain.skip_to(5).unwrap();

        assert!(skipped.is_empty());
        assert_eq!(final_chain.generation(), 5);
        assert_eq!(final_chain.as_bytes(), chain.as_bytes());
    }
}
