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
#[non_exhaustive]
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
    #[error("Invalid social network username format")]
    InvalidSocialUsername,
}

/// Maximum length for field values.
pub const MAX_VALUE_LENGTH: usize = 1000;

/// Maximum length for field labels (#192).
pub const MAX_LABEL_LENGTH: usize = 64;

/// Maximum length for field notes.
pub const MAX_FIELD_NOTE_LEN: usize = 500;

/// Type of contact field.
///
/// Note: Social networks are handled generically via `Social` type.
/// The label field identifies the specific network (e.g., "Twitter", "LinkedIn").
/// Future: A configurable social network registry will provide validation rules
/// and identity verification methods for each network.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FieldType {
    Phone,
    Email,
    Social,
    Address,
    Website,
    Birthday,
    Custom,
}

impl FieldType {
    /// Returns `true` if this is a social network field type.
    pub fn is_social(&self) -> bool {
        matches!(self, FieldType::Social)
    }

    /// Resolves a human-friendly alias to a `FieldType` and optional label.
    ///
    /// Social network aliases (e.g. "twitter", "instagram") return the label
    /// to use for the field. Generic aliases (e.g. "phone", "email") return
    /// `None` for the label.
    ///
    /// Returns `None` if the alias is not recognized.
    pub fn from_alias(s: &str) -> Option<(FieldType, Option<String>)> {
        match s.to_lowercase().as_str() {
            "phone" | "tel" | "telephone" => Some((FieldType::Phone, None)),
            "email" | "mail" => Some((FieldType::Email, None)),
            "address" | "addr" | "home" => Some((FieldType::Address, None)),
            "website" | "web" | "url" => Some((FieldType::Website, None)),
            "birthday" | "bday" | "dob" => Some((FieldType::Birthday, None)),
            "social" => Some((FieldType::Social, None)),
            "twitter" | "x" => Some((FieldType::Social, Some("Twitter".to_string()))),
            "instagram" | "ig" => Some((FieldType::Social, Some("Instagram".to_string()))),
            "linkedin" => Some((FieldType::Social, Some("LinkedIn".to_string()))),
            "github" | "gh" => Some((FieldType::Social, Some("GitHub".to_string()))),
            "custom" | "other" | "note" => Some((FieldType::Custom, None)),
            _ => None,
        }
    }
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
    /// Private per-field annotation (never sent to other contacts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

impl ContactField {
    /// Creates a new contact field with a generated ID.
    ///
    /// `now` is the Unix-epoch timestamp stamped into
    /// `updated_at`. Production callers source it from
    /// `storage.clock().unix_seconds()` (Storage-side),
    /// `self.clock.unix_seconds()` (Vauchi-side), or
    /// `engine.vauchi().clock().unix_seconds()`
    /// (platform-side). Tests pass any fixed value.
    pub fn new(field_type: FieldType, label: &str, value: &str, now: u64) -> Self {
        let rand_id: [u8; 8] = crate::crypto::random_bytes();
        let id = hex::encode(rand_id);

        ContactField {
            id,
            field_type,
            label: normalize_text(label),
            value: normalize_text(value),
            updated_at: now,
            note: None,
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
        // `take(MAX)` is a no-op for inputs with ≤ MAX chars and a
        // truncation otherwise — no length-comparison branch needed.
        self.label = normalize_text(label)
            .chars()
            .take(MAX_LABEL_LENGTH)
            .collect();
    }

    /// Returns the field value.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Returns the timestamp of the last update (Unix seconds).
    pub fn updated_at(&self) -> u64 {
        self.updated_at
    }

    /// Sets the field value and stamps `updated_at`.
    pub fn set_value(&mut self, value: &str, now: u64) {
        self.value = normalize_text(value);
        self.updated_at = now;
    }

    /// Returns the private note, if any.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// Builder: set a private note on this field. Truncates to 500 chars.
    pub fn with_note(mut self, note: String) -> Self {
        self.set_note(Some(note));
        self
    }

    /// Mutably set (or clear) the private note. Truncates to 500 chars.
    pub fn set_note(&mut self, note: Option<String>) {
        // `take(MAX)` is a no-op for inputs with ≤ MAX chars and a
        // truncation otherwise — no length-comparison branch needed.
        self.note = match note {
            None => None,
            Some(n) if n.is_empty() => None,
            Some(n) => Some(n.chars().take(MAX_FIELD_NOTE_LEN).collect()),
        };
    }

    /// Returns a clone with all private fields stripped.
    ///
    /// Used before building outbound card deltas — notes must NEVER
    /// appear in data sent to other contacts.
    pub fn strip_private(&self) -> Self {
        let mut stripped = self.clone();
        stripped.note = None;
        stripped
    }

    /// Validates the field value based on its type.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.value.len() > MAX_VALUE_LENGTH {
            return Err(ValidationError::ValueTooLong {
                max: MAX_VALUE_LENGTH,
            });
        }

        match self.field_type {
            FieldType::Phone => self.validate_phone(),
            FieldType::Email => self.validate_email(),
            FieldType::Website => self.validate_website(),
            FieldType::Birthday => self.validate_birthday(),
            FieldType::Social => self.validate_social(),
            _ => Ok(()), // Address, Custom accept any value
        }
    }

    /// Validates phone number format.
    fn validate_phone(&self) -> Result<(), ValidationError> {
        let value = &self.value;

        // Reasonable max phone length (international with formatting)
        if value.len() > 30 {
            return Err(ValidationError::InvalidPhone);
        }

        let digit_count = value.chars().filter(|c| c.is_ascii_digit()).count();
        if digit_count < 7 {
            return Err(ValidationError::InvalidPhone);
        }

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

        if !value.contains('@') {
            return Err(ValidationError::InvalidEmail);
        }

        let parts: Vec<&str> = value.split('@').collect();
        if parts.len() != 2 {
            return Err(ValidationError::InvalidEmail);
        }

        let local = parts[0];
        let domain = parts[1];

        if local.is_empty() {
            return Err(ValidationError::InvalidEmail);
        }

        // Domain must be non-empty. We intentionally allow domains
        // without a dot (e.g. `user@localhost`) — only emptiness is
        // a hard reject.
        if domain.is_empty() {
            return Err(ValidationError::InvalidEmail);
        }

        Ok(())
    }

    /// Validates website URL format.
    fn validate_website(&self) -> Result<(), ValidationError> {
        let value = self.value.trim();
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

        if value.len() != 10 || value.as_bytes()[4] != b'-' || value.as_bytes()[7] != b'-' {
            return Err(ValidationError::InvalidEmail); // Reuse validation error for invalid birthday
        }

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

        if !(1..=12).contains(&month) {
            return Err(ValidationError::InvalidEmail);
        }

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

    /// Validates social network usernames when the label identifies a known network.
    /// Unknown networks accept any non-empty value.
    fn validate_social(&self) -> Result<(), ValidationError> {
        let username = self.value.trim_start_matches('@');
        match self.label.to_lowercase().as_str() {
            "twitter" | "x" => {
                // Twitter: max 15 chars, alphanumeric + underscore only (ADR-spec: social-registry)
                if username.len() > 15
                    || !username
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                    || username.is_empty()
                {
                    return Err(ValidationError::InvalidSocialUsername);
                }
            }
            "github" | "gh" => {
                // GitHub: max 39 chars, alphanumeric + hyphens, no leading/trailing/consecutive hyphens
                if username.is_empty()
                    || username.len() > 39
                    || username.starts_with('-')
                    || username.ends_with('-')
                    || username.contains("--")
                    || !username
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-')
                {
                    return Err(ValidationError::InvalidSocialUsername);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
