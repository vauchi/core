//! Social Profile Validation
//!
//! Implements crowd-sourced validation of social profiles.
//! Users can verify that a contact's social profile belongs to them,
//! building trust through consensus.
//!
//! ## How It Works
//!
//! 1. Alice claims her Twitter handle is "@alice" in her contact card
//! 2. Bob and Carol personally know Alice and verify this is correct
//! 3. They each create a signed validation record
//! 4. When Dave views Alice's card, he sees "Verified by Bob, Carol"
//! 5. The trust level increases with more validations
//!
//! ## Trust Levels
//!
//! - **Unverified** (0 validations): Grey indicator
//! - **Low Confidence** (1 validation): Yellow indicator
//! - **Partial Confidence** (2-4 validations): Light green indicator
//! - **High Confidence** (5+ validations): Green indicator

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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
            .unwrap()
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
            .unwrap()
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
}

/// Trust level based on validation count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustLevel {
    /// No validations yet.
    Unverified,
    /// 1 validation.
    LowConfidence,
    /// 2-4 validations.
    PartialConfidence,
    /// 5+ validations.
    HighConfidence,
}

impl TrustLevel {
    /// Determines trust level from validation count.
    pub fn from_count(count: usize) -> Self {
        match count {
            0 => TrustLevel::Unverified,
            1 => TrustLevel::LowConfidence,
            2..=4 => TrustLevel::PartialConfidence,
            _ => TrustLevel::HighConfidence,
        }
    }

    /// Returns a human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            TrustLevel::Unverified => "unverified",
            TrustLevel::LowConfidence => "low confidence",
            TrustLevel::PartialConfidence => "partial confidence",
            TrustLevel::HighConfidence => "verified",
        }
    }

    /// Returns a color indicator for UI.
    pub fn color(&self) -> &'static str {
        match self {
            TrustLevel::Unverified => "grey",
            TrustLevel::LowConfidence => "yellow",
            TrustLevel::PartialConfidence => "light_green",
            TrustLevel::HighConfidence => "green",
        }
    }
}

/// Aggregated validation status for a social field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationStatus {
    /// Total number of validations.
    pub count: usize,
    /// Trust level based on count.
    pub trust_level: TrustLevel,
    /// IDs of validators (for display, may be partial).
    pub validator_ids: Vec<String>,
    /// Whether the current user has validated this.
    pub validated_by_me: bool,
    /// Current field value (validations are invalidated if this changes).
    pub field_value: String,
}

impl ValidationStatus {
    /// Creates a new validation status.
    pub fn new(field_value: &str) -> Self {
        Self {
            count: 0,
            trust_level: TrustLevel::Unverified,
            validator_ids: Vec::new(),
            validated_by_me: false,
            field_value: field_value.to_string(),
        }
    }

    /// Updates the status from a list of validations.
    pub fn from_validations(
        validations: &[ProfileValidation],
        field_value: &str,
        my_id: Option<&str>,
        blocked_ids: &HashSet<String>,
    ) -> Self {
        // Filter to valid validations (matching field value, not blocked)
        let valid_validations: Vec<_> = validations
            .iter()
            .filter(|v| v.field_value == field_value && !blocked_ids.contains(&v.validator_id))
            .collect();

        let count = valid_validations.len();
        let trust_level = TrustLevel::from_count(count);

        let validator_ids: Vec<String> = valid_validations
            .iter()
            .map(|v| v.validator_id.clone())
            .collect();

        let validated_by_me = my_id
            .map(|id| validator_ids.contains(&id.to_string()))
            .unwrap_or(false);

        Self {
            count,
            trust_level,
            validator_ids,
            validated_by_me,
            field_value: field_value.to_string(),
        }
    }

    /// Formats a display string for the validation status.
    pub fn display(&self, known_names: &std::collections::HashMap<String, String>) -> String {
        if self.count == 0 {
            return "Not verified".to_string();
        }

        // Find known names among validators
        let known: Vec<&String> = self
            .validator_ids
            .iter()
            .filter_map(|id| known_names.get(id))
            .take(2)
            .collect();

        let others = self.count.saturating_sub(known.len());

        match (known.len(), others) {
            (0, n) => format!(
                "Verified by {} {}",
                n,
                if n == 1 { "person" } else { "people" }
            ),
            (1, 0) => format!("Verified by {}", known[0]),
            (1, n) => format!(
                "Verified by {} and {} {}",
                known[0],
                n,
                if n == 1 { "other" } else { "others" }
            ),
            (2, 0) => format!("Verified by {} and {}", known[0], known[1]),
            (2, n) => format!(
                "Verified by {}, {} and {} {}",
                known[0],
                known[1],
                n,
                if n == 1 { "other" } else { "others" }
            ),
            _ => format!("Verified by {} people", self.count),
        }
    }
}

/// Custom serde for fixed-size signature arrays.
mod signature_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            bytes,
        ))
    }

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
