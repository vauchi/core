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

pub use field::{ContactField, FieldType};
pub use uri::{is_allowed_scheme, ContactAction};
pub use validation::ValidationError;

use ring::rand::SystemRandom;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum number of fields per contact card.
pub const MAX_FIELDS: usize = 25;

/// Maximum display name length.
pub const MAX_DISPLAY_NAME_LENGTH: usize = 100;

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
}
