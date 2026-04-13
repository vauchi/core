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

pub mod catalog;
pub mod vcard;
pub mod vcard_import;

pub use catalog::{CatalogEntry, FieldCategory, FieldTypeCatalog};
pub use field::{ContactField, FieldType, ValidationError};
pub use uri::{
    ContactAction, is_allowed_scheme, is_blocked_scheme, is_safe_url, is_valid_phone,
    is_valid_relay_url,
};

use image::ImageReader;
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use crate::text::normalize_text;
use crate::types::VisibilityRules;
use thiserror::Error;

/// Maximum number of fields per contact card.
pub const MAX_FIELDS: usize = 200;

/// Maximum display name length.
pub const MAX_DISPLAY_NAME_LENGTH: usize = 100;

/// Maximum serialized card size in bytes (64 KB).
pub const MAX_CARD_SIZE_BYTES: usize = 65536;

/// Maximum avatar data size in bytes (32 KB, ADR-042).
pub const MAX_AVATAR_SIZE: usize = 32_768;

/// Maximum dimension (width or height) for avatar images.
const MAX_AVATAR_DIMENSION: u32 = 512;

/// Normalize any supported image (PNG, JPEG, BMP, WebP) to WebP <= `MAX_AVATAR_SIZE`.
///
/// Decodes the input, resizes if either dimension exceeds `MAX_AVATAR_DIMENSION`,
/// and encodes as WebP. If the result still exceeds the size budget, the dimension
/// is halved repeatedly until the output fits or the dimension reaches 32 px.
pub fn normalize_avatar(data: &[u8]) -> Result<Vec<u8>, ContactCardError> {
    if data.is_empty() {
        return Err(ContactCardError::AvatarInvalidFormat);
    }

    let reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|_| ContactCardError::AvatarInvalidFormat)?;

    let img = reader
        .decode()
        .map_err(|_| ContactCardError::AvatarInvalidFormat)?;

    let mut dim = MAX_AVATAR_DIMENSION;

    loop {
        let resized = if img.width() > dim || img.height() > dim {
            img.resize(dim, dim, image::imageops::FilterType::Lanczos3)
        } else {
            img.clone()
        };

        let mut buf = Cursor::new(Vec::new());
        resized
            .write_to(&mut buf, image::ImageFormat::WebP)
            .map_err(|_| ContactCardError::AvatarInvalidFormat)?;

        let output = buf.into_inner();
        if output.len() <= MAX_AVATAR_SIZE || dim <= 32 {
            return Ok(output);
        }

        dim /= 2;
    }
}

/// Current ContactCard schema version.
/// Incremented when the serialized format changes.
/// v0 = legacy (no schema_version field), v1 = first versioned format.
pub const CURRENT_CARD_SCHEMA_VERSION: u32 = 1;

/// Contact card errors.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ContactCardError {
    #[error("Display name cannot be empty")]
    EmptyDisplayName,
    #[error("Display name too long (max 100 characters)")]
    DisplayNameTooLong,
    #[error("Maximum number of fields reached ({MAX_FIELDS})")]
    MaxFieldsReached,
    #[error("Field not found")]
    FieldNotFound,
    #[error("Avatar too large (max {max} bytes, got {size} bytes)")]
    AvatarTooLarge { max: usize, size: usize },
    #[error("Unsupported avatar image format (expected PNG, JPEG, BMP, or WebP)")]
    AvatarInvalidFormat,
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
    /// Schema version for forward/backward compatibility.
    /// Legacy data (pre-versioning) defaults to 0 via `#[serde(default)]`.
    #[serde(default)]
    schema_version: u32,
    /// Unique identifier for this card.
    id: String,
    /// User's display name.
    display_name: String,
    /// Contact information fields.
    fields: Vec<ContactField>,
    /// Optional avatar image data (WebP, max 32 KB per ADR-042).
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    avatar: Option<Vec<u8>>,
    /// Optional local nickname annotation (max 100 chars, never shared).
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    nickname: Option<String>,
    /// Per-field visibility rules for the own card.
    /// In no-group mode: `Everyone` = visible, `Nobody` = hidden.
    /// In groups mode: group membership determines visibility (this is ignored).
    /// Default: all fields hidden (privacy-first).
    #[serde(default, skip_serializing_if = "VisibilityRules::is_empty")]
    field_visibility: VisibilityRules,
}

impl ContactCard {
    /// Creates a new contact card with the given display name.
    pub fn new(display_name: &str) -> Self {
        let rand_id: [u8; 16] = crate::crypto::random_bytes();
        let id = hex::encode(rand_id);

        ContactCard {
            schema_version: CURRENT_CARD_SCHEMA_VERSION,
            id,
            display_name: normalize_text(display_name),
            fields: Vec::new(),
            avatar: None,
            nickname: None,
            field_visibility: VisibilityRules::new(),
        }
    }

    /// Returns the schema version of this card's serialized format.
    /// 0 = legacy (pre-versioning), 1 = first versioned format.
    pub fn schema_version(&self) -> u32 {
        self.schema_version
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
        let normalized = normalize_text(name);
        if normalized.is_empty() {
            return Err(ContactCardError::EmptyDisplayName);
        }
        if normalized.chars().count() > MAX_DISPLAY_NAME_LENGTH {
            return Err(ContactCardError::DisplayNameTooLong);
        }
        self.display_name = normalized;
        Ok(())
    }

    /// Returns the optional nickname annotation stored in the card blob.
    ///
    /// **Deprecated:** This field lives inside the serialized card and risks
    /// overwrite on card updates. Use the contact-level nickname instead:
    /// `Vauchi::get_contact_nickname()` / `Vauchi::set_contact_nickname()`.
    pub fn nickname(&self) -> Option<&str> {
        self.nickname.as_deref()
    }

    /// Sets the local nickname annotation in the card blob (max 100 chars).
    ///
    /// **Deprecated:** Use `Vauchi::set_contact_nickname()` instead.
    /// Card-level nickname risks overwrite on card updates.
    pub fn set_nickname(&mut self, nickname: &str) {
        let normalized = normalize_text(nickname);
        if normalized.is_empty() {
            self.nickname = None;
        } else {
            let truncated = normalized
                .chars()
                .take(MAX_DISPLAY_NAME_LENGTH)
                .collect::<String>();
            self.nickname = Some(truncated);
        }
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

        // Enforce single birthday constraint (Phase 3)
        if field.field_type() == FieldType::Birthday
            && self
                .fields
                .iter()
                .any(|f| f.field_type() == FieldType::Birthday)
        {
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

    /// Updates a field's private note by ID. Pass `None` to clear the note.
    pub fn update_field_note(
        &mut self,
        field_id: &str,
        note: Option<String>,
    ) -> Result<(), ContactCardError> {
        let field = self
            .fields
            .iter_mut()
            .find(|f| f.id() == field_id)
            .ok_or(ContactCardError::FieldNotFound)?;

        field.set_note(note);
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
        self.field_visibility.remove(field_id);
        Ok(())
    }

    /// Validates that the serialized card size is within the maximum limit.
    pub fn validate_size(&self) -> Result<(), ContactCardError> {
        let json =
            serde_json::to_vec(self).map_err(|e| ContactCardError::Serialization(e.to_string()))?;
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
    /// Accepts any supported image format (PNG, JPEG, BMP, WebP).
    /// The image is normalized to WebP <= 32 KB (ADR-042).
    pub fn set_avatar(&mut self, data: Vec<u8>) -> Result<(), ContactCardError> {
        let webp = normalize_avatar(&data)?;
        self.avatar = Some(webp);
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

    /// Returns the per-field visibility rules.
    pub fn field_visibility(&self) -> &VisibilityRules {
        &self.field_visibility
    }

    /// Returns mutable access to per-field visibility rules.
    pub fn field_visibility_mut(&mut self) -> &mut VisibilityRules {
        &mut self.field_visibility
    }

    /// Returns whether a field is visible to everyone (no-group mode).
    ///
    /// Uses privacy-first default: fields without an explicit rule are hidden.
    pub fn is_field_shown(&self, field_id: &str) -> bool {
        self.field_visibility.is_explicitly_everyone(field_id)
    }

    /// Sets whether a field is shown to everyone (no-group mode).
    /// When true, sets `Everyone`. When false, sets `Nobody`.
    ///
    /// Silently ignores the operation if `field_id` does not exist on this card
    /// (e.g. stale ID after field deletion). This prevents orphaned IDs in
    /// `field_visibility`.
    pub fn set_field_shown(&mut self, field_id: &str, shown: bool) {
        if shown && !self.fields.iter().any(|f| f.id() == field_id) {
            return;
        }
        if shown {
            self.field_visibility.set_everyone(field_id);
        } else {
            self.field_visibility.set_nobody(field_id);
        }
    }
}
