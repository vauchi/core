// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Content Encryption Key (CEK) for crypto-shredding.
//!
//! Per-contact symmetric key that controls at-rest readability of a contact card
//! stored on the recipient's device. Destroying this key renders the card
//! permanently unreadable (crypto-shredding).
//!
//! Security role: at-rest erasure only. Transport forward secrecy is provided
//! by the Double Ratchet. The CEK adds the ability to remotely render cards
//! unreadable by destroying the key.

use super::encryption::{EncryptionError, SymmetricKey, decrypt, encrypt};

/// Per-contact content encryption key for at-rest card protection.
///
/// Controls readability of a contact card stored on the recipient's device.
/// Destroying this key renders the card permanently unreadable (crypto-shredding).
///
/// Delegates to [`encrypt`]/[`decrypt`] (XChaCha20-Poly1305).
/// Zeroized on drop via the inner [`SymmetricKey`].
#[derive(zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct ContentEncryptionKey(SymmetricKey);

impl ContentEncryptionKey {
    /// Generate a new random CEK.
    pub fn generate() -> Self {
        Self(SymmetricKey::generate())
    }

    /// Encrypt plaintext with this CEK (XChaCha20-Poly1305).
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        encrypt(&self.0, plaintext)
    }

    /// Decrypt ciphertext with this CEK.
    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        decrypt(&self.0, ciphertext)
    }

    /// Serialize for transmission (32 raw bytes).
    pub fn to_bytes(&self) -> [u8; 32] {
        *self.0.as_bytes()
    }

    /// Deserialize from received bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(SymmetricKey::from_bytes(bytes))
    }
}

/// Security: Clone is required because `Contact` derives `Clone` and holds
/// `Option<ContentEncryptionKey>`. Both copies are individually zeroized on drop
/// via `ZeroizeOnDrop` on the inner `SymmetricKey`, but two copies exist in memory
/// simultaneously while both are alive. Removing Clone from `Contact` is tracked
/// as a future refactoring (#90).
impl Clone for ContentEncryptionKey {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl std::fmt::Debug for ContentEncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContentEncryptionKey")
            .field("key", &"[REDACTED]")
            .finish()
    }
}
