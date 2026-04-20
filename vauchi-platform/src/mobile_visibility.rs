// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility operations and labels for mobile.

use super::VauchiPlatform;
use super::error::MobileError;
use super::types::{MobileVisibilityLabel, MobileVisibilityLabelDetail};

#[uniffi::export]
impl VauchiPlatform {
    // === Visibility Operations ===

    /// Hide field from contact.
    pub fn hide_field_from_contact(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&contact_id)?
            .ok_or_else(|| MobileError::Other {
                message: format!("Contact not found: {}", contact_id.clone()),
            })?;

        let card = storage.load_own_card()?.ok_or(MobileError::Other {
            message: "Identity not found".to_string(),
        })?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput {
                field: String::new(),
                message: format!("Field not found: {}", field_label),
            })?;

        contact
            .visibility_rules_mut()
            .ok_or(MobileError::InvalidInput {
                field: String::new(),
                message: "Visibility rules require an exchanged contact".to_string(),
            })?
            .set_nobody(field.id());
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Show field to contact.
    pub fn show_field_to_contact(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&contact_id)?
            .ok_or_else(|| MobileError::Other {
                message: format!("Contact not found: {}", contact_id.clone()),
            })?;

        let card = storage.load_own_card()?.ok_or(MobileError::Other {
            message: "Identity not found".to_string(),
        })?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput {
                field: String::new(),
                message: format!("Field not found: {}", field_label),
            })?;

        contact
            .visibility_rules_mut()
            .ok_or(MobileError::InvalidInput {
                field: String::new(),
                message: "Visibility rules require an exchanged contact".to_string(),
            })?
            .set_everyone(field.id());
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Check if field is visible to contact.
    pub fn is_field_visible_to_contact(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;

        let contact = storage
            .load_contact(&contact_id)?
            .ok_or_else(|| MobileError::Other {
                message: format!("Contact not found: {}", contact_id.clone()),
            })?;

        let card = storage.load_own_card()?.ok_or(MobileError::Other {
            message: "Identity not found".to_string(),
        })?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput {
                field: String::new(),
                message: format!("Field not found: {}", field_label),
            })?;

        Ok(contact
            .visibility_rules()
            .is_some_and(|rules| rules.can_see(field.id(), &contact_id)))
    }

    // === Visibility Labels ===

    /// List all visibility labels.
    pub fn list_labels(&self) -> Result<Vec<MobileVisibilityLabel>, MobileError> {
        let storage = self.open_storage()?;
        let labels = storage.load_all_groups()?;
        Ok(labels.iter().map(MobileVisibilityLabel::from).collect())
    }

    /// Create a new visibility label.
    pub fn create_label(&self, name: String) -> Result<MobileVisibilityLabel, MobileError> {
        let storage = self.open_storage()?;
        let label = storage.create_group(&name)?;
        Ok(MobileVisibilityLabel::from(&label))
    }

    /// Get a label by ID with full details.
    pub fn get_label(&self, label_id: String) -> Result<MobileVisibilityLabelDetail, MobileError> {
        let storage = self.open_storage()?;
        let label = storage.load_group(&label_id)?;
        Ok(MobileVisibilityLabelDetail::from(&label))
    }

    /// Rename a label.
    pub fn rename_label(&self, label_id: String, new_name: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.rename_group(&label_id, &new_name)?;
        Ok(())
    }

    /// Delete a label.
    pub fn delete_label(&self, label_id: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.delete_group(&label_id)?;
        Ok(())
    }

    /// Add a contact to a label.
    pub fn add_contact_to_group(
        &self,
        label_id: String,
        contact_id: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.add_contact_to_group(&label_id, &contact_id)?;
        Ok(())
    }

    /// Remove a contact from a label.
    pub fn remove_contact_from_group(
        &self,
        label_id: String,
        contact_id: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.remove_contact_from_group(&label_id, &contact_id)?;
        Ok(())
    }

    /// Get all labels that contain a contact.
    pub fn get_groups_for_contact(
        &self,
        contact_id: String,
    ) -> Result<Vec<MobileVisibilityLabel>, MobileError> {
        let storage = self.open_storage()?;
        let labels = storage.get_groups_for_contact(&contact_id)?;
        Ok(labels.iter().map(MobileVisibilityLabel::from).collect())
    }

    /// Set whether a field is visible to contacts in a label.
    pub fn set_group_field_visibility(
        &self,
        label_id: String,
        field_label: String,
        is_visible: bool,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let card = storage.load_own_card()?.ok_or(MobileError::Other {
            message: "Identity not found".to_string(),
        })?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput {
                field: String::new(),
                message: format!("Field not found: {}", field_label),
            })?;

        storage.set_group_field_visibility(&label_id, field.id(), is_visible)?;
        Ok(())
    }

    /// Set a per-contact override for field visibility.
    ///
    /// Per-contact overrides take precedence over label-based visibility.
    pub fn set_contact_field_override(
        &self,
        contact_id: String,
        field_label: String,
        is_visible: bool,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let card = storage.load_own_card()?.ok_or(MobileError::Other {
            message: "Identity not found".to_string(),
        })?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput {
                field: String::new(),
                message: format!("Field not found: {}", field_label),
            })?;

        storage.save_contact_override(&contact_id, field.id(), is_visible)?;
        Ok(())
    }

    /// Remove a per-contact override for field visibility.
    pub fn remove_contact_field_override(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let card = storage.load_own_card()?.ok_or(MobileError::Other {
            message: "Identity not found".to_string(),
        })?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput {
                field: String::new(),
                message: format!("Field not found: {}", field_label),
            })?;

        storage.delete_contact_override(&contact_id, field.id())?;
        Ok(())
    }

    /// Get suggested default labels.
    pub fn get_suggested_labels(&self) -> Vec<String> {
        vauchi_core::SUGGESTED_LABELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}
