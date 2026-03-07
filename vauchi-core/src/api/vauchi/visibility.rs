// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility labels, field validation, and incoming validation processing.

use crate::network::Transport;

use super::super::error::{VauchiError, VauchiResult};
use super::super::events::VauchiEvent;
use super::Vauchi;

impl<T: Transport> Vauchi<T> {
    // === Visibility Labels ===

    /// Lists all visibility labels.
    pub fn list_labels(&self) -> VauchiResult<Vec<crate::contact::VisibilityLabel>> {
        Ok(self.storage.load_all_labels()?)
    }

    /// Creates a new visibility label.
    pub fn create_label(&self, name: &str) -> VauchiResult<crate::contact::VisibilityLabel> {
        Ok(self.storage.create_label(name)?)
    }

    /// Renames a visibility label.
    pub fn rename_label(&self, label_id: &str, new_name: &str) -> VauchiResult<()> {
        Ok(self.storage.rename_label(label_id, new_name)?)
    }

    /// Sets or clears the per-group display name override.
    ///
    /// When set, contacts in this group see this name instead of the
    /// user's default display name. Pass `None` to clear.
    /// Persists the updated label to storage.
    pub fn set_label_display_name_override(
        &self,
        label_id: &str,
        name_override: Option<&str>,
    ) -> VauchiResult<()> {
        let mut label = self.storage.load_label(label_id)?;
        label
            .set_display_name_override(name_override)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        self.storage.save_label(&label)?;
        Ok(())
    }

    /// Deletes a visibility label.
    ///
    /// Contacts in the label remain in the contact list; they just lose
    /// their label membership.
    ///
    /// When deleting the last label (transition to no-group mode), visible
    /// fields from the deleted label are migrated to `shown_fields` on the
    /// own card so field visibility is preserved.
    pub fn delete_label(&self, label_id: &str) -> VauchiResult<()> {
        // Load the label before deletion to capture its visible fields
        let label = self.storage.load_label(label_id)?;
        let visible_fields = label.visible_fields().clone();

        // Delete the label from storage
        self.storage.delete_label(label_id)?;

        // Check if this was the last label
        let remaining_labels = self.storage.load_all_labels()?;
        if remaining_labels.is_empty() && !visible_fields.is_empty() {
            // Transitioning to no-group mode:
            // Migrate visible fields from the deleted label to shown_fields
            if let Some(mut card) = self.storage.load_own_card()? {
                for field_id in &visible_fields {
                    card.set_field_shown(field_id, true);
                }
                self.storage.save_own_card(&card)?;
            }
        }

        Ok(())
    }

    /// Gets a visibility label by ID.
    pub fn get_label(&self, label_id: &str) -> VauchiResult<crate::contact::VisibilityLabel> {
        Ok(self.storage.load_label(label_id)?)
    }

    /// Gets all contacts that are members of a visibility label.
    ///
    /// Loads the label, extracts its contact IDs, then loads the actual
    /// `Contact` objects. Contacts that no longer exist in storage are
    /// silently skipped.
    pub fn get_label_members(&self, label_id: &str) -> VauchiResult<Vec<crate::contact::Contact>> {
        let label = self.storage.load_label(label_id)?;
        let mut members = Vec::new();
        for contact_id in label.contacts() {
            if let Some(contact) = self.storage.load_contact(contact_id)? {
                members.push(contact);
            }
        }
        Ok(members)
    }

    /// Adds a contact to a visibility label.
    pub fn add_contact_to_label(&self, label_id: &str, contact_id: &str) -> VauchiResult<()> {
        Ok(self.storage.add_contact_to_label(label_id, contact_id)?)
    }

    /// Removes a contact from a visibility label.
    pub fn remove_contact_from_label(&self, label_id: &str, contact_id: &str) -> VauchiResult<()> {
        Ok(self
            .storage
            .remove_contact_from_label(label_id, contact_id)?)
    }

    /// Gets all labels that contain a specific contact.
    pub fn get_labels_for_contact(
        &self,
        contact_id: &str,
    ) -> VauchiResult<Vec<crate::contact::VisibilityLabel>> {
        Ok(self.storage.get_labels_for_contact(contact_id)?)
    }

    /// Sets field visibility for a label.
    ///
    /// When `is_visible` is true, contacts in this label will see the field.
    /// When false, the field is hidden from contacts in this label.
    pub fn set_label_field_visibility(
        &self,
        label_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> VauchiResult<()> {
        Ok(self
            .storage
            .set_label_field_visibility(label_id, field_id, is_visible)?)
    }

    /// Sets a per-contact visibility override for a field.
    ///
    /// Per-contact overrides take precedence over label-based visibility.
    pub fn set_contact_visibility_override(
        &self,
        contact_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> VauchiResult<()> {
        Ok(self
            .storage
            .save_contact_override(contact_id, field_id, is_visible)?)
    }

    /// Removes a per-contact visibility override.
    pub fn remove_contact_visibility_override(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> VauchiResult<()> {
        Ok(self.storage.delete_contact_override(contact_id, field_id)?)
    }

    /// Gets all per-contact visibility overrides for a contact.
    pub fn get_contact_visibility_overrides(
        &self,
        contact_id: &str,
    ) -> VauchiResult<std::collections::HashMap<String, bool>> {
        Ok(self.storage.load_contact_overrides(contact_id)?)
    }

    /// Determines the effective visibility of a field for a contact.
    ///
    /// Returns visibility determined by (in priority order):
    /// 1. Per-contact override (if set)
    /// 2. Label membership (visible if contact is in any label that shows this field)
    /// 3. Contact's VisibilityRules (the default field visibility)
    pub fn get_effective_field_visibility(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> VauchiResult<bool> {
        // Load the contact's visibility rules as fallback
        let contact = self
            .storage
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::NotFound(format!("contact: {}", contact_id)))?;

        // Check per-contact override first
        let overrides = self.storage.load_contact_overrides(contact_id)?;
        if let Some(&is_visible) = overrides.get(field_id) {
            return Ok(is_visible);
        }

        // Check if any label containing this contact shows this field
        let labels = self.storage.get_labels_for_contact(contact_id)?;
        for label in labels {
            if label.is_field_visible(field_id) {
                return Ok(true);
            }
        }

        // Fall back to contact's default visibility rules
        // Note: The visibility rules determine what this contact can see of *our* card
        // We use their contact_id to check if they're in the allowed list
        Ok(contact.visibility_rules().can_see(field_id, contact_id))
    }

    // === Field Validation Operations ===

    /// Validates a contact's field.
    ///
    /// Creates a cryptographically signed validation record that attests
    /// the current user believes the field value belongs to the contact.
    ///
    /// # Arguments
    /// * `contact_id` - The contact whose field is being validated
    /// * `field_id` - The field name (e.g., "twitter", "email")
    /// * `field_value` - The current value of the field
    ///
    /// # Returns
    /// The created validation record
    pub fn validate_field(
        &self,
        contact_id: &str,
        field_id: &str,
        field_value: &str,
    ) -> VauchiResult<crate::social::ProfileValidation> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        // Check we're not validating our own field
        let my_id = hex::encode(identity.signing_public_key());
        if contact_id == my_id {
            return Err(VauchiError::InvalidState(
                "Cannot validate your own field".into(),
            ));
        }

        // Check we haven't already validated this field
        let validator_id = hex::encode(identity.signing_public_key());
        if self
            .storage
            .has_validated(contact_id, field_id, &validator_id)?
        {
            return Err(VauchiError::InvalidState(
                "You have already validated this field".into(),
            ));
        }

        // Create signed validation
        let validation = crate::social::ProfileValidation::create_signed(
            identity,
            field_id,
            field_value,
            contact_id,
        );

        // Store it
        self.storage.save_validation(&validation)?;

        // Queue for delivery to the validated contact.
        // Queue failure does NOT fail the validation itself — local storage succeeded.
        if let Ok(validation_bytes) = serde_json::to_vec(&validation) {
            let sync_manager = crate::sync::state::SyncManager::new(&self.storage);
            let _ = sync_manager.queue_validation_delivery(contact_id, validation_bytes);
        }

        Ok(validation)
    }

    /// Gets the validation status for a contact's field.
    ///
    /// Returns aggregated validation information including count, trust level,
    /// and whether the current user has validated this field.
    pub fn get_field_validation_status(
        &self,
        contact_id: &str,
        field_id: &str,
        field_value: &str,
    ) -> VauchiResult<crate::social::ValidationStatus> {
        let validations = self
            .storage
            .load_validations_for_field(contact_id, field_id)?;

        // Get current user's ID if available
        let my_id = self
            .identity
            .as_ref()
            .map(|id| hex::encode(id.signing_public_key()));

        let contacts = self.storage.list_contacts()?;

        // Load blocked contact IDs to exclude their validations
        let blocked: std::collections::HashSet<String> = contacts
            .iter()
            .filter(|c| c.is_blocked())
            .map(|c| c.id().to_string())
            .collect();

        // Build validator metadata from contacts for trust weighting
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        let validator_meta: std::collections::HashMap<String, crate::social::ValidatorMeta> =
            contacts
                .iter()
                .map(|c| {
                    let age_days = (now.saturating_sub(c.exchange_timestamp())) / 86400;
                    (
                        c.id().to_string(),
                        crate::social::ValidatorMeta {
                            contact_age_days: age_days,
                            fingerprint_verified: c.is_fingerprint_verified(),
                        },
                    )
                })
                .collect();

        let status = crate::social::ValidationStatus::from_validations_weighted(
            &validations,
            field_value,
            my_id.as_deref(),
            &blocked,
            &validator_meta,
        );

        Ok(status)
    }

    /// Revokes the current user's validation of a field.
    ///
    /// Returns true if a validation was revoked, false if none existed.
    pub fn revoke_field_validation(&self, contact_id: &str, field_id: &str) -> VauchiResult<bool> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let validator_id = hex::encode(identity.signing_public_key());
        let deleted = self
            .storage
            .delete_validation(contact_id, field_id, &validator_id)?;

        // Queue revocation for delivery to the contact.
        // Queue failure does NOT fail the revocation itself — local deletion succeeded.
        if deleted {
            let revocation_info = serde_json::json!({
                "contact_id": contact_id,
                "field_id": field_id,
                "validator_id": validator_id,
            });
            if let Ok(revocation_bytes) = serde_json::to_vec(&revocation_info) {
                let sync_manager = crate::sync::state::SyncManager::new(&self.storage);
                let _ = sync_manager.queue_validation_revocation(contact_id, revocation_bytes);
            }
        }

        Ok(deleted)
    }

    /// Lists all validations made by the current user.
    ///
    /// Returns a list of all fields the user has validated, sorted by
    /// validation timestamp (most recent first).
    pub fn list_my_validations(&self) -> VauchiResult<Vec<crate::social::ProfileValidation>> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let validator_id = hex::encode(identity.signing_public_key());
        let validations = self.storage.load_validations_by_validator(&validator_id)?;

        Ok(validations)
    }

    /// Checks if the current user has validated a specific field.
    pub fn has_validated_field(&self, contact_id: &str, field_id: &str) -> VauchiResult<bool> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let validator_id = hex::encode(identity.signing_public_key());
        let validated = self
            .storage
            .has_validated(contact_id, field_id, &validator_id)?;

        Ok(validated)
    }

    // === Incoming Validation Processing ===

    /// Processes an incoming validation record from a contact.
    ///
    /// Verifies the Ed25519 signature against the sender's public key,
    /// then stores the validation if valid. Idempotent — duplicate
    /// deliveries are handled by the UNIQUE constraint on storage.
    pub fn process_incoming_validation(
        &self,
        sender_contact_id: &str,
        validation_bytes: &[u8],
    ) -> VauchiResult<()> {
        // 1. Deserialize the ProfileValidation from JSON bytes
        let validation: crate::social::ProfileValidation = serde_json::from_slice(validation_bytes)
            .map_err(|e| VauchiError::Serialization(e.to_string()))?;

        // 2. Verify the validator_id matches the sender (prevents forwarding attacks)
        if validation.validator_id() != sender_contact_id {
            return Err(VauchiError::InvalidState(
                "validator_id does not match sender contact".into(),
            ));
        }

        // 3. Look up the sender contact to get their public key
        let contact = self
            .get_contact(sender_contact_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(sender_contact_id.to_string()))?;

        // 4. Verify the Ed25519 signature against the sender's public key
        if !validation.verify(contact.public_key()) {
            return Err(VauchiError::SignatureInvalid);
        }

        // 5. Store the validation (save_validation is idempotent via INSERT OR REPLACE)
        self.storage.save_validation(&validation)?;

        // 6. Dispatch FieldValidated event
        self.events.dispatch(VauchiEvent::FieldValidated {
            contact_id: validation.contact_id().unwrap_or_default().to_string(),
            field_id: validation.field_id().to_string(),
            validator_id: validation.validator_id().to_string(),
        });

        Ok(())
    }

    /// Processes an incoming validation revocation from a contact.
    ///
    /// Verifies the sender matches the validator_id in the revocation,
    /// then deletes the validation from storage.
    ///
    /// Returns `true` if a validation was deleted, `false` if none existed.
    pub fn process_incoming_revocation(
        &self,
        sender_contact_id: &str,
        revocation_bytes: &[u8],
    ) -> VauchiResult<bool> {
        // 1. Deserialize the revocation payload
        let revocation: serde_json::Value = serde_json::from_slice(revocation_bytes)
            .map_err(|e| VauchiError::Serialization(e.to_string()))?;

        let contact_id = revocation["contact_id"]
            .as_str()
            .ok_or_else(|| VauchiError::Serialization("missing contact_id in revocation".into()))?;
        let field_id = revocation["field_id"]
            .as_str()
            .ok_or_else(|| VauchiError::Serialization("missing field_id in revocation".into()))?;
        let validator_id = revocation["validator_id"].as_str().ok_or_else(|| {
            VauchiError::Serialization("missing validator_id in revocation".into())
        })?;

        // 2. Verify sender_contact_id matches the validator_id
        if validator_id != sender_contact_id {
            return Err(VauchiError::InvalidState(
                "validator_id does not match sender contact".into(),
            ));
        }

        // 3. Delete the validation from storage
        let deleted = self
            .storage
            .delete_validation(contact_id, field_id, validator_id)?;

        // 4. Dispatch FieldValidationRevoked event only if something was actually deleted
        if deleted {
            self.events.dispatch(VauchiEvent::FieldValidationRevoked {
                contact_id: contact_id.to_string(),
                field_id: field_id.to_string(),
                validator_id: validator_id.to_string(),
            });
        }

        Ok(deleted)
    }
}
