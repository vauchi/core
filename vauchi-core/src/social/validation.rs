// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Field Validation
//!
//! Provides the signed validation record type (`ProfileValidation`) for
//! attesting that a contact field value is authentic. Field confidence
//! is now derived from the viewer's fingerprint verification status
//! (see `is_fingerprint_verified()`), not from cross-user scoring.

use serde::{Deserialize, Serialize};

use crate::Identity;

/// A validation record for a social profile field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileValidation {
    /// ID of the field being validated.
    field_id: String,
    /// Value of the field at time of validation.
    field_value: String,
    /// Contact ID of the validator.
    validator_id: String,
    /// Timestamp when validation was created.
    validated_at: u64,
    /// Signature from the validator's identity key.
    #[serde(with = "signature_serde")]
    signature: [u8; 64],
}

impl ProfileValidation {
    /// Creates a new validation record.
    pub fn new(field_id: &str, field_value: &str, validator_id: &str, signature: [u8; 64]) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        Self {
            field_id: field_id.to_string(),
            field_value: field_value.to_string(),
            validator_id: validator_id.to_string(),
            validated_at: now,
            signature,
        }
    }

    /// Returns the field ID being validated.
    pub fn field_id(&self) -> &str {
        &self.field_id
    }

    /// Returns the field value at time of validation.
    pub fn field_value(&self) -> &str {
        &self.field_value
    }

    /// Returns the validator's contact ID.
    pub fn validator_id(&self) -> &str {
        &self.validator_id
    }

    /// Returns the validation timestamp.
    pub fn validated_at(&self) -> u64 {
        self.validated_at
    }

    /// Returns the signature.
    pub fn signature(&self) -> &[u8; 64] {
        &self.signature
    }

    /// Returns the bytes to be signed for this validation.
    pub fn signable_bytes(&self) -> Vec<u8> {
        format!(
            "VAUCHI_VALIDATION:{}:{}:{}:{}",
            self.field_id, self.field_value, self.validator_id, self.validated_at
        )
        .into_bytes()
    }

    /// Verifies the validation signature against a public key.
    pub fn verify(&self, public_key: &[u8; 32]) -> bool {
        use crate::crypto::{PublicKey, Signature};

        let message = self.signable_bytes();
        let signature = Signature::from_bytes(self.signature);
        let pubkey = PublicKey::from_bytes(*public_key);

        pubkey.verify(&message, &signature)
    }

    /// Creates a signed validation record using the validator's identity.
    ///
    /// This is the primary way to create validations - the signature is
    /// created using the validator's Ed25519 signing key.
    pub fn create_signed(
        identity: &Identity,
        field_id: &str,
        field_value: &str,
        contact_id: &str,
    ) -> Self {
        let validator_id = hex::encode(identity.signing_public_key());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        // Full field ID includes contact_id prefix
        let full_field_id = format!("{}:{}", contact_id, field_id);

        // Create the message to sign (must match signable_bytes format)
        let message = format!(
            "VAUCHI_VALIDATION:{}:{}:{}:{}",
            full_field_id, field_value, validator_id, now
        );

        // Sign with the identity's signing key
        let signature = identity.sign(message.as_bytes());

        Self {
            field_id: full_field_id,
            field_value: field_value.to_string(),
            validator_id,
            validated_at: now,
            signature: *signature.as_bytes(),
        }
    }

    /// Returns the contact ID this validation is for.
    ///
    /// The field_id is formatted as "contact_id:field_name".
    pub fn contact_id(&self) -> Option<&str> {
        self.field_id.split(':').next()
    }

    /// Returns the field name being validated.
    pub fn field_name(&self) -> Option<&str> {
        self.field_id.split(':').nth(1)
    }

    /// Creates a validation from stored data.
    ///
    /// Used when loading validations from the database.
    pub fn from_stored(
        field_id: &str,
        field_value: &str,
        validator_id: &str,
        validated_at: u64,
        signature: [u8; 64],
    ) -> Self {
        Self {
            field_id: field_id.to_string(),
            field_value: field_value.to_string(),
            validator_id: validator_id.to_string(),
            validated_at,
            signature,
        }
    }
}

/// Custom serde for fixed-size signature arrays.
mod signature_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serializes a 64-byte signature to a base64-encoded string for social validation payloads.
    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            bytes,
        ))
    }

    /// Deserializes a 64-byte signature from a base64-encoded string.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &s)
            .map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid signature length"))
    }
}
