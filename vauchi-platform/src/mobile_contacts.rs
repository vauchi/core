// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact card, contact CRUD, hidden contacts, field validation, and social network operations.

use vauchi_core::ContactField;

use super::VauchiPlatform;
use super::error::MobileError;
use super::types::{
    MobileContact, MobileContactCard, MobileFieldNote, MobileFieldType, MobileFieldValidation,
    MobileSocialNetwork, MobileValidationStatus,
};

#[uniffi::export]
impl VauchiPlatform {
    // === Contact Card Operations ===

    /// Get own contact card.
    pub fn get_own_card(&self) -> Result<MobileContactCard, MobileError> {
        let storage = self.open_storage()?;
        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        Ok(MobileContactCard::from(&card))
    }

    /// Add field to own card.
    pub fn add_field(
        &self,
        field_type: MobileFieldType,
        label: String,
        value: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;

        let field = ContactField::new(field_type.into(), &label, &value);
        card.add_field(field)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;

        storage.save_own_card(&card)?;
        Ok(())
    }

    /// Update field value.
    pub fn update_field(&self, label: String, new_value: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;

        let field_id = card
            .fields()
            .iter()
            .find(|f| f.label() == label)
            .ok_or_else(|| MobileError::InvalidInput(format!("Field '{}' not found", label)))?
            .id()
            .to_string();

        card.update_field_value(&field_id, &new_value)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;

        storage.save_own_card(&card)?;
        Ok(())
    }

    /// Remove field from card.
    pub fn remove_field(&self, label: String) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;

        let field_id = match card.fields().iter().find(|f| f.label() == label) {
            Some(f) => f.id().to_string(),
            None => return Ok(false),
        };

        card.remove_field(&field_id)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;
        storage.save_own_card(&card)?;

        Ok(true)
    }

    /// Set display name.
    pub fn set_display_name(&self, name: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;

        card.set_display_name(&name)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;
        storage.save_own_card(&card)?;

        Ok(())
    }

    // === Contact Operations ===

    /// List all contacts.
    pub fn list_contacts(&self) -> Result<Vec<MobileContact>, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        Ok(contacts.iter().map(MobileContact::from).collect())
    }

    /// Get single contact by ID.
    pub fn get_contact(&self, id: String) -> Result<Option<MobileContact>, MobileError> {
        let storage = self.open_storage()?;
        let contact = storage.load_contact(&id)?;
        Ok(contact.as_ref().map(MobileContact::from))
    }

    /// Search contacts using SQL-level search.
    pub fn search_contacts(&self, query: String) -> Result<Vec<MobileContact>, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.search_contacts(&query)?;
        Ok(contacts.iter().map(MobileContact::from).collect())
    }

    /// Get contact count.
    pub fn contact_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        Ok(contacts.len() as u32)
    }

    /// Remove contact.
    pub fn remove_contact(&self, id: String) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;
        let removed = storage.delete_contact(&id)?;
        Ok(removed)
    }

    /// Verify contact fingerprint.
    pub fn verify_contact(&self, id: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&id)?
            .ok_or_else(|| MobileError::ContactNotFound(id.clone()))?;

        contact
            .mark_fingerprint_verified()
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Mark a contact as trusted for recovery.
    ///
    /// Blocked contacts cannot be trusted for recovery.
    pub fn trust_contact_for_recovery(&self, id: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&id)?
            .ok_or_else(|| MobileError::ContactNotFound(id.clone()))?;

        if contact.is_blocked() {
            return Err(MobileError::InvalidInput(
                "Blocked contacts cannot be trusted for recovery".to_string(),
            ));
        }

        contact
            .trust_for_recovery()
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Remove recovery trust from a contact.
    pub fn untrust_contact_for_recovery(&self, id: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&id)?
            .ok_or_else(|| MobileError::ContactNotFound(id.clone()))?;

        contact
            .untrust_for_recovery()
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Get the number of contacts trusted for recovery.
    pub fn trusted_contact_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        let count = contacts.iter().filter(|c| c.is_recovery_trusted()).count();
        Ok(count as u32)
    }

    // === Personal Notes Operations ===

    /// Save a personal note for a contact.
    ///
    /// Notes are private ("your eyes only") — they are never sent to the contact.
    /// An empty string clears the note.
    pub fn set_contact_note(&self, contact_id: String, note: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.save_personal_notes(&contact_id, note.as_bytes())?;
        Ok(())
    }

    /// Load the personal note for a contact, if any.
    ///
    /// Returns `None` if no note has been saved.
    pub fn get_contact_note(&self, contact_id: String) -> Result<Option<String>, MobileError> {
        let storage = self.open_storage()?;
        let bytes = storage.load_personal_notes(&contact_id)?;
        Ok(bytes.and_then(|b| String::from_utf8(b).ok()))
    }

    /// Delete the personal note for a contact.
    ///
    /// No error is returned if no note existed.
    pub fn delete_contact_note(&self, contact_id: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.delete_personal_notes(&contact_id)?;
        Ok(())
    }

    // === Contact Field Notes Operations ===

    /// Save a private note on a specific field of a contact.
    ///
    /// Notes are private ("your eyes only") — they are never sent to the contact.
    /// An empty string clears the note.
    pub fn set_contact_field_note(
        &self,
        contact_id: String,
        field_id: String,
        note: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.save_contact_field_note(&contact_id, &field_id, note.as_bytes())?;
        Ok(())
    }

    /// Load all private field notes for a contact.
    ///
    /// Returns a list of `(field_id, note)` pairs. Fields with no note are omitted.
    pub fn get_contact_field_notes(
        &self,
        contact_id: String,
    ) -> Result<Vec<MobileFieldNote>, MobileError> {
        let storage = self.open_storage()?;
        let map = storage.load_contact_field_notes(&contact_id)?;
        let mut notes: Vec<MobileFieldNote> = map
            .into_iter()
            .filter_map(|(field_id, bytes)| {
                String::from_utf8(bytes)
                    .ok()
                    .map(|note| MobileFieldNote { field_id, note })
            })
            .collect();
        // Stable ordering for deterministic output
        notes.sort_by(|a, b| a.field_id.cmp(&b.field_id));
        Ok(notes)
    }

    /// Delete the private note on a specific field of a contact.
    ///
    /// No error is returned if no note existed.
    pub fn delete_contact_field_note(
        &self,
        contact_id: String,
        field_id: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.delete_contact_field_note(&contact_id, &field_id)?;
        Ok(())
    }

    // === Proposal Trust Operations ===

    /// Mark a contact as trusted for simplified contact proposals.
    ///
    /// This is a local-only flag — the contact is never informed of their trust status.
    pub fn set_proposal_trusted(
        &self,
        contact_id: String,
        trusted: bool,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&contact_id)?
            .ok_or_else(|| MobileError::ContactNotFound(contact_id.clone()))?;

        contact
            .set_proposal_trusted(trusted)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;
        storage.save_contact(&contact)?;

        Ok(())
    }

    // === Hidden Contact Operations ===

    /// Hides a contact from the main contact list.
    ///
    /// Hidden contacts provide plausible deniability - they only appear
    /// via secret access (gesture, PIN, or special settings navigation).
    /// Routes through the Vauchi API to ensure `ContactHidden` events are dispatched.
    pub fn hide_contact(&self, contact_id: String) -> Result<(), MobileError> {
        let vauchi = self.open_vauchi()?;
        vauchi.hide_contact(&contact_id)?;
        Ok(())
    }

    /// Unhides a contact, making it visible in the main contact list again.
    /// Routes through the Vauchi API to ensure `ContactUnhidden` events are dispatched.
    pub fn unhide_contact(&self, contact_id: String) -> Result<(), MobileError> {
        let vauchi = self.open_vauchi()?;
        vauchi.unhide_contact(&contact_id)?;
        Ok(())
    }

    /// Lists all hidden contacts.
    /// Routes through the Vauchi API for consistency with hide/unhide operations.
    pub fn list_hidden_contacts(&self) -> Result<Vec<MobileContact>, MobileError> {
        let vauchi = self.open_vauchi()?;
        let contacts = vauchi.list_hidden_contacts()?;
        Ok(contacts.iter().map(MobileContact::from).collect())
    }

    /// List contacts with pagination.
    pub fn list_contacts_paginated(
        &self,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<MobileContact>, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts_paginated(offset as usize, limit as usize)?;
        Ok(contacts.iter().map(MobileContact::from).collect())
    }

    // === Social Networks ===

    /// List available social networks.
    pub fn list_social_networks(&self) -> Vec<MobileSocialNetwork> {
        self.social_registry
            .all()
            .iter()
            .map(|sn| MobileSocialNetwork {
                id: sn.id().to_string(),
                display_name: sn.display_name().to_string(),
                url_template: sn.profile_url_template().to_string(),
            })
            .collect()
    }

    /// Search social networks.
    pub fn search_social_networks(&self, query: String) -> Vec<MobileSocialNetwork> {
        self.social_registry
            .search(&query)
            .iter()
            .map(|sn| MobileSocialNetwork {
                id: sn.id().to_string(),
                display_name: sn.display_name().to_string(),
                url_template: sn.profile_url_template().to_string(),
            })
            .collect()
    }

    /// Get profile URL for a social field.
    pub fn get_profile_url(&self, network_id: String, username: String) -> Option<String> {
        self.social_registry.profile_url(&network_id, &username)
    }

    // === Field Validation Operations ===

    /// Validate a contact's field.
    ///
    /// Creates a cryptographically signed validation record attesting
    /// that you believe this field value belongs to this contact.
    /// Returns the created validation.
    pub fn validate_field(
        &self,
        contact_id: String,
        field_id: String,
        field_value: String,
    ) -> Result<MobileFieldValidation, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let my_id = hex::encode(identity.signing_public_key());
        if contact_id == my_id {
            return Err(MobileError::InvalidInput(
                "Cannot validate your own field".to_string(),
            ));
        }

        let validator_id = hex::encode(identity.signing_public_key());
        if storage.has_validated(&contact_id, &field_id, &validator_id)? {
            return Err(MobileError::InvalidInput(
                "You have already validated this field".to_string(),
            ));
        }

        let validation = vauchi_core::social::ProfileValidation::create_signed(
            &identity,
            &field_id,
            &field_value,
            &contact_id,
        );

        storage.save_validation(&validation)?;

        Ok(MobileFieldValidation::from(&validation))
    }

    /// Get validation status for a contact's field.
    ///
    /// Returns aggregated validation information including count, trust level,
    /// and whether you have validated this field.
    pub fn get_field_validation_status(
        &self,
        contact_id: String,
        field_id: String,
        field_value: String,
    ) -> Result<MobileValidationStatus, MobileError> {
        let storage = self.open_storage()?;
        let validations = storage.load_validations_for_field(&contact_id, &field_id)?;

        let my_id = {
            let data = self.identity_data.lock().unwrap();
            if data.is_some() {
                match self.get_identity() {
                    Ok(identity) => Some(hex::encode(identity.signing_public_key())),
                    Err(_) => None,
                }
            } else {
                None
            }
        };

        let blocked = std::collections::HashSet::new();
        let status = vauchi_core::social::ValidationStatus::from_validations(
            &validations,
            &field_value,
            my_id.as_deref(),
            &blocked,
        );

        Ok(MobileValidationStatus::from(&status))
    }

    /// Revoke your validation of a contact's field.
    ///
    /// Returns true if a validation was revoked, false if you hadn't validated.
    pub fn revoke_field_validation(
        &self,
        contact_id: String,
        field_id: String,
    ) -> Result<bool, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let validator_id = hex::encode(identity.signing_public_key());
        let deleted = storage.delete_validation(&contact_id, &field_id, &validator_id)?;

        Ok(deleted)
    }

    /// List all validations you have made.
    ///
    /// Returns a list of all fields you have validated, sorted by
    /// validation timestamp (most recent first).
    pub fn list_my_validations(&self) -> Result<Vec<MobileFieldValidation>, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let validator_id = hex::encode(identity.signing_public_key());
        let validations = storage.load_validations_by_validator(&validator_id)?;

        Ok(validations
            .iter()
            .map(MobileFieldValidation::from)
            .collect())
    }

    /// Check if you have validated a specific field.
    pub fn has_validated_field(
        &self,
        contact_id: String,
        field_id: String,
    ) -> Result<bool, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let validator_id = hex::encode(identity.signing_public_key());
        let validated = storage.has_validated(&contact_id, &field_id, &validator_id)?;

        Ok(validated)
    }

    /// Get the validation count for a field (quick check without full status).
    pub fn get_field_validation_count(
        &self,
        contact_id: String,
        field_id: String,
    ) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let count = storage.count_validations_for_field(&contact_id, &field_id)?;
        Ok(count as u32)
    }
}
