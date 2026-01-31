// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Card Management Module
//!
//! Handles contact card creation, fields, and validation.

#[cfg(feature = "testing")]
pub mod field;
#[cfg(not(feature = "testing"))]
mod field;

#[cfg(feature = "testing")]
pub mod uri;
#[cfg(not(feature = "testing"))]
mod uri;

#[cfg(feature = "testing")]
pub mod validation;
#[cfg(not(feature = "testing"))]
mod validation;

pub mod vcard;

pub use field::{ContactField, FieldType};
pub use uri::{is_allowed_scheme, is_blocked_scheme, is_safe_url, ContactAction};
pub use validation::ValidationError;

use ring::rand::SystemRandom;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum number of fields per contact card.
pub const MAX_FIELDS: usize = 25;

/// Maximum display name length.
pub const MAX_DISPLAY_NAME_LENGTH: usize = 100;

/// Maximum serialized card size in bytes (64 KB).
pub const MAX_CARD_SIZE_BYTES: usize = 65536;

/// Maximum avatar data size in bytes (256 KB).
pub const MAX_AVATAR_SIZE: usize = 262144;

/// Contact card errors.
#[derive(Error, Debug)]
pub enum ContactCardError {
    #[error("Display name cannot be empty")]
    EmptyDisplayName,
    #[error("Display name too long (max 100 characters)")]
    DisplayNameTooLong,
    #[error("Maximum number of fields reached (25)")]
    MaxFieldsReached,
    #[error("Field not found")]
    FieldNotFound,
    #[error("Avatar too large (max {max} bytes, got {size} bytes)")]
    AvatarTooLarge { max: usize, size: usize },
    #[error("Card too large (max {max} bytes, got {size} bytes)")]
    CardTooLarge { max: usize, size: usize },
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Validation error: {0}")]
    Validation(#[from] ValidationError),
}

/// A user's contact card containing personal information fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContactCard {
    /// Unique identifier for this card.
    id: String,
    /// User's display name.
    display_name: String,
    /// Contact information fields.
    fields: Vec<ContactField>,
    /// Optional avatar image data (max 256 KB).
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    avatar: Option<Vec<u8>>,
}

impl ContactCard {
    /// Creates a new contact card with the given display name.
    pub fn new(display_name: &str) -> Self {
        let rng = SystemRandom::new();
        let random_bytes = ring::rand::generate::<[u8; 16]>(&rng)
            .expect("System RNG should not fail")
            .expose();
        let id = hex::encode(random_bytes);

        ContactCard {
            id,
            display_name: display_name.to_string(),
            fields: Vec::new(),
            avatar: None,
        }
    }

    /// Returns the card's unique ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the display name.
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Sets the display name.
    pub fn set_display_name(&mut self, name: &str) -> Result<(), ContactCardError> {
        if name.is_empty() {
            return Err(ContactCardError::EmptyDisplayName);
        }
        if name.len() > MAX_DISPLAY_NAME_LENGTH {
            return Err(ContactCardError::DisplayNameTooLong);
        }
        self.display_name = name.to_string();
        Ok(())
    }

    /// Returns all fields.
    pub fn fields(&self) -> &[ContactField] {
        &self.fields
    }

    /// Returns mutable access to all fields.
    pub fn fields_mut(&mut self) -> &mut Vec<ContactField> {
        &mut self.fields
    }

    /// Adds a field to the card.
    pub fn add_field(&mut self, field: ContactField) -> Result<(), ContactCardError> {
        if self.fields.len() >= MAX_FIELDS {
            return Err(ContactCardError::MaxFieldsReached);
        }

        // Validate the field before adding
        field.validate()?;

        self.fields.push(field);
        Ok(())
    }

    /// Updates a field's value by ID.
    pub fn update_field_value(
        &mut self,
        field_id: &str,
        value: &str,
    ) -> Result<(), ContactCardError> {
        let field = self
            .fields
            .iter_mut()
            .find(|f| f.id() == field_id)
            .ok_or(ContactCardError::FieldNotFound)?;

        field.set_value(value);
        field.validate()?;
        Ok(())
    }

    /// Updates a field's label by ID.
    pub fn update_field_label(
        &mut self,
        field_id: &str,
        label: &str,
    ) -> Result<(), ContactCardError> {
        let field = self
            .fields
            .iter_mut()
            .find(|f| f.id() == field_id)
            .ok_or(ContactCardError::FieldNotFound)?;

        field.set_label(label);
        Ok(())
    }

    /// Removes a field from the card by ID.
    pub fn remove_field(&mut self, field_id: &str) -> Result<(), ContactCardError> {
        let index = self
            .fields
            .iter()
            .position(|f| f.id() == field_id)
            .ok_or(ContactCardError::FieldNotFound)?;

        self.fields.remove(index);
        Ok(())
    }

    /// Validates that the serialized card size is within the maximum limit.
    pub fn validate_size(&self) -> Result<(), ContactCardError> {
        let json = serde_json::to_vec(self).map_err(|e| {
            ContactCardError::Serialization(e.to_string())
        })?;
        let size = json.len();
        if size > MAX_CARD_SIZE_BYTES {
            return Err(ContactCardError::CardTooLarge {
                max: MAX_CARD_SIZE_BYTES,
                size,
            });
        }
        Ok(())
    }

    /// Reorders fields according to the given ID order.
    ///
    /// Fields whose IDs appear in `field_ids` are placed first, in the given order.
    /// Fields not in the list are appended at the end in their original order.
    /// Returns an error if any ID in `field_ids` does not match an existing field.
    pub fn reorder_fields(&mut self, field_ids: &[&str]) -> Result<(), ContactCardError> {
        // Validate that all provided IDs exist
        for &id in field_ids {
            if !self.fields.iter().any(|f| f.id() == id) {
                return Err(ContactCardError::FieldNotFound);
            }
        }

        let mut reordered: Vec<ContactField> = Vec::with_capacity(self.fields.len());

        // First, add fields in the specified order
        for &id in field_ids {
            if let Some(pos) = self.fields.iter().position(|f| f.id() == id) {
                reordered.push(self.fields.remove(pos));
            }
        }

        // Then append remaining fields in their original order
        reordered.append(&mut self.fields);

        self.fields = reordered;
        Ok(())
    }

    /// Sets the avatar image data.
    ///
    /// Returns an error if the data exceeds the maximum avatar size (256 KB).
    pub fn set_avatar(&mut self, data: Vec<u8>) -> Result<(), ContactCardError> {
        if data.len() > MAX_AVATAR_SIZE {
            return Err(ContactCardError::AvatarTooLarge {
                max: MAX_AVATAR_SIZE,
                size: data.len(),
            });
        }
        self.avatar = Some(data);
        Ok(())
    }

    /// Returns the avatar image data, if set.
    pub fn avatar(&self) -> Option<&[u8]> {
        self.avatar.as_deref()
    }

    /// Clears the avatar image data.
    pub fn clear_avatar(&mut self) {
        self.avatar = None;
    }
}
