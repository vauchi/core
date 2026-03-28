// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Key rotation detection for delivery error handling.
//!
//! Detects when a recipient's cryptographic key has changed between send attempts,
//! which may indicate key rotation or a security event requiring user verification.

use subtle::ConstantTimeEq;

/// Error type for key rotation detection.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyRotationError {
    /// Recipient's signing key has changed.
    RecipientKeyChanged { contact_id: String },
}

/// Detects key changes for delivery reliability and security.
///
/// Used to identify when a recipient's key has rotated or changed unexpectedly,
/// allowing delivery subsystem to report meaningful errors to the user.
#[derive(Debug, Clone)]
pub struct KeyRotationDetector;

impl KeyRotationDetector {
    /// Creates a new key rotation detector.
    pub fn new() -> Self {
        KeyRotationDetector
    }

    /// Checks if a recipient's key has changed.
    ///
    /// # Arguments
    /// * `contact_id` - Recipient's contact ID
    /// * `known_key` - Previously known public key (32 bytes)
    /// * `current_key` - Current public key from recipient (32 bytes)
    ///
    /// # Returns
    /// * `Ok(())` if keys match
    /// * `Err(KeyRotationError::RecipientKeyChanged)` if keys differ
    pub fn check_key_consistency(
        &self,
        contact_id: &str,
        known_key: &[u8; 32],
        current_key: &[u8; 32],
    ) -> Result<(), KeyRotationError> {
        if bool::from(known_key.ct_eq(current_key)) {
            Ok(())
        } else {
            Err(KeyRotationError::RecipientKeyChanged {
                contact_id: contact_id.to_string(),
            })
        }
    }
}

impl Default for KeyRotationDetector {
    fn default() -> Self {
        Self::new()
    }
}
