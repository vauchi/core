// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Field Types
//!
//! Handles individual contact fields like phone, email, social media, etc.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::text::normalize_text;

/// Validation error types for contact field values.
#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Invalid phone number format")]
    InvalidPhone,
    #[error("Invalid email format")]
    InvalidEmail,
    #[error("Invalid URL format")]
    InvalidUrl,
    #[error("Value too long (max {max} characters)")]
    ValueTooLong { max: usize },
    #[error("Value cannot be empty")]
    EmptyValue,
}

/// Maximum length for field values.
pub const MAX_VALUE_LENGTH: usize = 1000;

/// Maximum length for field labels (#192).
pub const MAX_LABEL_LENGTH: usize = 64;

/// Type of contact field.
///
/// Note: Social networks are handled generically via `Social` type.
/// The label field identifies the specific network (e.g., "Twitter", "LinkedIn").
/// Future: A configurable social network registry will provide validation rules
/// and identity verification methods for each network.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FieldType {
    Phone,
    Email,
    Social,
    Address,
    Website,
    Birthday,
    Custom,
}

/// Returns the current Unix timestamp in seconds.
fn now_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System clock before UNIX epoch")
        .as_secs()
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
    /// Timestamp of the last update (Unix seconds). Defaults to 0 for backward compatibility.
    #[serde(default)]
    updated_at: u64,
}

impl ContactField {
    /// Creates a new contact field with a generated ID.
    pub fn new(field_type: FieldType, label: &str, value: &str) -> Self {
        let rand_id: [u8; 8] = crate::crypto::random_bytes();
        let id = hex::encode(rand_id);

        ContactField {
            id,
            field_type,
            label: normalize_text(label),
            value: normalize_text(value),
            updated_at: now_timestamp(),
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

    /// Sets the field label. Truncates to MAX_LABEL_LENGTH chars (#192).
    pub fn set_label(&mut self, label: &str) {
        let normalized = normalize_text(label);
        if normalized.chars().count() > MAX_LABEL_LENGTH {
            self.label = normalized.chars().take(MAX_LABEL_LENGTH).collect();
        } else {
            self.label = normalized;
        }
    }

    /// Returns the field value.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Returns the timestamp of the last update (Unix seconds).
    pub fn updated_at(&self) -> u64 {
        self.updated_at
    }

    /// Sets the field value and updates the timestamp.
    pub fn set_value(&mut self, value: &str) {
        self.value = normalize_text(value);
        self.updated_at = now_timestamp();
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
            FieldType::Website => self.validate_website(),
            FieldType::Birthday => self.validate_birthday(),
            _ => Ok(()), // Social, Address, Custom accept any value
        }
    }

    /// Validates phone number format.
    fn validate_phone(&self) -> Result<(), ValidationError> {
        let value = &self.value;

        // Reasonable max phone length (international with formatting)
        if value.len() > 30 {
            return Err(ValidationError::InvalidPhone);
        }

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

    /// Validates website URL format.
    fn validate_website(&self) -> Result<(), ValidationError> {
        let value = self.value.trim();
        // Must start with http:// or https://, or contain a dot (domain)
        if value.starts_with("http://") || value.starts_with("https://") {
            return Ok(());
        }
        if value.contains('.') && !value.contains(' ') {
            return Ok(());
        }
        Err(ValidationError::InvalidUrl)
    }

    /// Validates ISO 8601 birthday format (YYYY-MM-DD) and checks date validity.
    fn validate_birthday(&self) -> Result<(), ValidationError> {
        let value = &self.value;

        // Check format: YYYY-MM-DD
        if value.len() != 10 || value.as_bytes()[4] != b'-' || value.as_bytes()[7] != b'-' {
            return Err(ValidationError::InvalidEmail); // Reuse validation error for invalid birthday
        }

        // Parse components
        let year_str = &value[0..4];
        let month_str = &value[5..7];
        let day_str = &value[8..10];

        let year: u16 = year_str
            .parse()
            .map_err(|_| ValidationError::InvalidEmail)?;
        let month: u8 = month_str
            .parse()
            .map_err(|_| ValidationError::InvalidEmail)?;
        let day: u8 = day_str.parse().map_err(|_| ValidationError::InvalidEmail)?;

        // Validate month range
        if !(1..=12).contains(&month) {
            return Err(ValidationError::InvalidEmail);
        }

        // Validate day range based on month
        let days_in_month = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if year.is_multiple_of(400) {
                    29
                } else if year.is_multiple_of(100) {
                    28
                } else if year.is_multiple_of(4) {
                    29
                } else {
                    28
                }
            }
            _ => return Err(ValidationError::InvalidEmail),
        };

        if !(1..=days_in_month).contains(&day) {
            return Err(ValidationError::InvalidEmail);
        }

        Ok(())
    }
}
