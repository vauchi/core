// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility labels and field visibility management.

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

impl Vauchi {
    // === Visibility Labels ===

    /// Lists all visibility labels.
    pub fn list_groups(&self) -> VauchiResult<Vec<crate::contact::Group>> {
        Ok(self.storage.load_all_groups()?)
    }

    /// Creates a new visibility label.
    pub fn create_group(&self, name: &str) -> VauchiResult<crate::contact::Group> {
        Ok(self.storage.create_group(name)?)
    }

    /// Renames a visibility label.
    pub fn rename_group(&self, label_id: &str, new_name: &str) -> VauchiResult<()> {
        Ok(self.storage.rename_group(label_id, new_name)?)
    }

    /// Sets or clears the per-group display name override.
    ///
    /// When set, contacts in this group see this name instead of the
    /// user's default display name. Pass `None` to clear.
    /// Persists the updated label to storage.
    pub fn set_group_display_name_override(
        &self,
        label_id: &str,
        name_override: Option<&str>,
    ) -> VauchiResult<()> {
        let mut label = self.storage.load_group(label_id)?;
        label
            .set_display_name_override(name_override)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        self.storage.save_group(&label)?;
        Ok(())
    }

    /// Deletes a visibility label.
    ///
    /// Contacts in the label remain in the contact list; they just lose
    /// their label membership.
    ///
    /// When deleting the last label (transition to no-group mode), visible
    /// fields from the deleted label are migrated to `field_visibility` on the
    /// own card so field visibility is preserved.
    ///
    /// Note: Only the *last-deleted* label's fields are migrated. If labels
    /// are deleted sequentially, fields assigned only to earlier-deleted
    /// labels won't carry over. This is intentional — deleting a label
    /// removes its field assignments. To preserve all field visibility when
    /// transitioning, delete all labels in a single operation or re-assign
    /// fields before deletion.
    pub fn delete_group(&self, label_id: &str) -> VauchiResult<()> {
        // Load the label before deletion to capture its visible fields
        let label = self.storage.load_group(label_id)?;
        let visible_fields = label.visible_fields().clone();

        // Delete the label from storage
        self.storage.delete_group(label_id)?;

        // Check if this was the last label
        let remaining_labels = self.storage.load_all_groups()?;
        if remaining_labels.is_empty() && !visible_fields.is_empty() {
            // Transitioning to no-group mode:
            // Migrate visible fields from the deleted label to field_visibility
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
    pub fn get_group(&self, label_id: &str) -> VauchiResult<crate::contact::Group> {
        Ok(self.storage.load_group(label_id)?)
    }

    /// Gets all contacts that are members of a visibility label.
    ///
    /// Loads the label, extracts its contact IDs, then loads the actual
    /// `Contact` objects. Contacts that no longer exist in storage are
    /// silently skipped.
    pub fn get_group_members(&self, label_id: &str) -> VauchiResult<Vec<crate::contact::Contact>> {
        let label = self.storage.load_group(label_id)?;
        let mut members = Vec::new();
        for contact_id in label.contacts() {
            if let Some(contact) = self.storage.load_contact(contact_id)? {
                members.push(contact);
            }
        }
        Ok(members)
    }

    /// Adds a contact to a visibility label.
    pub fn add_contact_to_group(&self, label_id: &str, contact_id: &str) -> VauchiResult<()> {
        Ok(self.storage.add_contact_to_group(label_id, contact_id)?)
    }

    /// Removes a contact from a visibility label.
    pub fn remove_contact_from_group(&self, label_id: &str, contact_id: &str) -> VauchiResult<()> {
        Ok(self
            .storage
            .remove_contact_from_group(label_id, contact_id)?)
    }

    /// Gets all labels that contain a specific contact.
    pub fn get_groups_for_contact(
        &self,
        contact_id: &str,
    ) -> VauchiResult<Vec<crate::contact::Group>> {
        Ok(self.storage.get_groups_for_contact(contact_id)?)
    }

    /// Sets field visibility for a label.
    ///
    /// When `is_visible` is true, contacts in this label will see the field.
    /// When false, the field is hidden from contacts in this label.
    pub fn set_group_field_visibility(
        &self,
        label_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> VauchiResult<()> {
        Ok(self
            .storage
            .set_group_field_visibility(label_id, field_id, is_visible)?)
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
        let labels = self.storage.get_groups_for_contact(contact_id)?;
        for label in labels {
            if label.is_field_visible(field_id) {
                return Ok(true);
            }
        }

        // Fall back to contact's default visibility rules
        // Note: The visibility rules determine what this contact can see of *our* card
        // We use their contact_id to check if they're in the allowed list
        // Imported contacts have no visibility rules; default to not visible
        Ok(contact
            .visibility_rules()
            .is_some_and(|rules| rules.can_see(field_id, contact_id)))
    }

    /// Toggles field visibility for a contact.
    ///
    /// If the field is currently visible to this contact, hides it.
    /// If hidden, makes it visible. Returns the new visibility state.
    pub fn toggle_field_visibility(
        &self,
        contact_id: &str,
        field_label: &str,
    ) -> VauchiResult<bool> {
        let mut contact = self
            .storage
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::InvalidState("Contact not found".into()))?;

        let rules = contact.visibility_rules().ok_or(VauchiError::InvalidState(
            "Visibility rules require an exchanged contact".into(),
        ))?;
        let current_can_see = rules.can_see(field_label, contact_id);

        if current_can_see {
            contact
                .visibility_rules_mut()
                .expect("checked above")
                .set_nobody(field_label);
        } else {
            contact
                .visibility_rules_mut()
                .expect("checked above")
                .set_everyone(field_label);
        }

        self.storage.save_contact(&contact)?;
        Ok(!current_can_see)
    }
}
