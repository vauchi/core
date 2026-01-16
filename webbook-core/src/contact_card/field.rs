//! Contact Field Types
//!
//! Handles individual contact fields like phone, email, social media, etc.

use super::ValidationError;
use serde::{Deserialize, Serialize};

/// Maximum length for field values.
pub const MAX_VALUE_LENGTH: usize = 1000;

/// Type of contact field.
///
/// Note: Social networks are handled generically via `Social` type.
/// The label field identifies the specific network (e.g., "Twitter", "LinkedIn").
/// Future: A configurable social network registry will provide validation rules
/// and identity verification methods for each network.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldType {
    Phone,
    Email,
    Social,
    Address,
    Website,
    Custom,
}

/// A single contact field (phone, email, etc.).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContactField {
    /// Unique identifier for this field.
    id: String,
    /// Type of field.
    field_type: FieldType,
    /// User-defined label (e.g., "Work", "Mobile").
    label: String,
    /// The actual value (phone number, email address, etc.).
    value: String,
}

impl ContactField {
    /// Creates a new contact field with a generated ID.
    pub fn new(field_type: FieldType, label: &str, value: &str) -> Self {
        use ring::rand::SystemRandom;

        let rng = SystemRandom::new();
        let random_bytes = ring::rand::generate::<[u8; 8]>(&rng)
            .expect("System RNG should not fail")
            .expose();
        let id = hex::encode(random_bytes);

        ContactField {
            id,
            field_type,
            label: label.to_string(),
            value: value.to_string(),
        }
    }

    /// Returns the field's unique ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the field type.
    pub fn field_type(&self) -> FieldType {
        self.field_type.clone()
    }

    /// Returns the field label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Sets the field label.
    pub fn set_label(&mut self, label: &str) {
        self.label = label.to_string();
    }

    /// Returns the field value.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Sets the field value.
    pub fn set_value(&mut self, value: &str) {
        self.value = value.to_string();
    }

    /// Validates the field value based on its type.
    pub fn validate(&self) -> Result<(), ValidationError> {
        // Check max length
        if self.value.len() > MAX_VALUE_LENGTH {
            return Err(ValidationError::ValueTooLong {
                max: MAX_VALUE_LENGTH,
            });
        }

        // Type-specific validation
        match self.field_type {
            FieldType::Phone => self.validate_phone(),
            FieldType::Email => self.validate_email(),
            _ => Ok(()), // Other types accept any value
        }
    }

    /// Validates phone number format.
    fn validate_phone(&self) -> Result<(), ValidationError> {
        let value = &self.value;

        // Must have at least some digits
        let digit_count = value.chars().filter(|c| c.is_ascii_digit()).count();
        if digit_count < 7 {
            return Err(ValidationError::InvalidPhone);
        }

        // Only allow digits, spaces, dashes, parentheses, and plus
        let valid_chars = value.chars().all(|c| {
            c.is_ascii_digit() || c == ' ' || c == '-' || c == '(' || c == ')' || c == '+'
        });

        if !valid_chars {
            return Err(ValidationError::InvalidPhone);
        }

        Ok(())
    }

    /// Validates email format.
    fn validate_email(&self) -> Result<(), ValidationError> {
        let value = &self.value;

        // Basic email validation: must have @ with text before and after
        if !value.contains('@') {
            return Err(ValidationError::InvalidEmail);
        }

        let parts: Vec<&str> = value.split('@').collect();
        if parts.len() != 2 {
            return Err(ValidationError::InvalidEmail);
        }

        let local = parts[0];
        let domain = parts[1];

        // Local part must not be empty
        if local.is_empty() {
            return Err(ValidationError::InvalidEmail);
        }

        // Domain must have at least one character and contain a dot (for TLD)
        // Or at least be non-empty
        if domain.is_empty() || !domain.contains('.') {
            // Allow domains without dots for flexibility (e.g., localhost)
            // But require at least some content
            if domain.is_empty() {
                return Err(ValidationError::InvalidEmail);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_field() {
        let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-1234");
        assert_eq!(field.field_type(), FieldType::Phone);
        assert_eq!(field.label(), "Mobile");
        assert_eq!(field.value(), "+1-555-1234");
    }

    #[test]
    fn test_validate_valid_phone() {
        let field = ContactField::new(FieldType::Phone, "Test", "+1-555-123-4567");
        assert!(field.validate().is_ok());
    }

    #[test]
    fn test_validate_valid_email() {
        let field = ContactField::new(FieldType::Email, "Test", "test@example.com");
        assert!(field.validate().is_ok());
    }
}
