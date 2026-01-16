//! Social Profile Validation
//!
//! Implements crowd-sourced validation of social profiles.
//! Users can verify that a contact's social profile belongs to them,
//! building trust through consensus.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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
            "WEBBOOK_VALIDATION:{}:{}:{}:{}",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trust_level_from_count() {
        assert_eq!(TrustLevel::from_count(0), TrustLevel::Unverified);
        assert_eq!(TrustLevel::from_count(1), TrustLevel::LowConfidence);
        assert_eq!(TrustLevel::from_count(2), TrustLevel::PartialConfidence);
        assert_eq!(TrustLevel::from_count(4), TrustLevel::PartialConfidence);
        assert_eq!(TrustLevel::from_count(5), TrustLevel::HighConfidence);
        assert_eq!(TrustLevel::from_count(100), TrustLevel::HighConfidence);
    }

    #[test]
    fn test_trust_level_labels() {
        assert_eq!(TrustLevel::Unverified.label(), "unverified");
        assert_eq!(TrustLevel::LowConfidence.label(), "low confidence");
        assert_eq!(TrustLevel::PartialConfidence.label(), "partial confidence");
        assert_eq!(TrustLevel::HighConfidence.label(), "verified");
    }

    #[test]
    fn test_validation_status_new() {
        let status = ValidationStatus::new("@alice");

        assert_eq!(status.count, 0);
        assert_eq!(status.trust_level, TrustLevel::Unverified);
        assert!(!status.validated_by_me);
        assert_eq!(status.field_value, "@alice");
    }

    #[test]
    fn test_validation_status_display_no_validations() {
        let status = ValidationStatus::new("@alice");
        let names = std::collections::HashMap::new();

        assert_eq!(status.display(&names), "Not verified");
    }

    #[test]
    fn test_validation_status_display_with_known_names() {
        let mut status = ValidationStatus::new("@alice");
        status.count = 3;
        status.validator_ids = vec!["bob".into(), "carol".into(), "dave".into()];

        let mut names = std::collections::HashMap::new();
        names.insert("bob".to_string(), "Bob".to_string());

        assert_eq!(status.display(&names), "Verified by Bob and 2 others");
    }

    #[test]
    fn test_validation_status_from_validations_filters_blocked() {
        let validations = vec![
            ProfileValidation::new("field1", "@alice", "bob", [0u8; 64]),
            ProfileValidation::new("field1", "@alice", "mallory", [0u8; 64]),
            ProfileValidation::new("field1", "@alice", "carol", [0u8; 64]),
        ];

        let mut blocked = HashSet::new();
        blocked.insert("mallory".to_string());

        let status = ValidationStatus::from_validations(&validations, "@alice", None, &blocked);

        assert_eq!(status.count, 2);
        assert!(!status.validator_ids.contains(&"mallory".to_string()));
    }

    #[test]
    fn test_validation_status_invalidated_on_value_change() {
        let validations = vec![ProfileValidation::new(
            "field1",
            "@alice_old",
            "bob",
            [0u8; 64],
        )];

        let status = ValidationStatus::from_validations(
            &validations,
            "@alice_new", // Value changed
            None,
            &HashSet::new(),
        );

        // Validation doesn't count because field value changed
        assert_eq!(status.count, 0);
    }
}
