// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`LabelStore`](super::LabelStore).

use std::collections::HashMap;

use super::{Storage, StorageError};
use crate::contact::{Group, GroupManager};

impl Storage {
    /// Saves a visibility label to storage (encrypted).
    ///
    /// Label name is encrypted at rest with HMAC for lookups (#128).
    pub fn save_group(&self, label: &Group) -> Result<(), StorageError> {
        self.labels().save_group(label)
    }
    /// Loads a visibility label by ID (decrypted).
    pub fn load_group(&self, label_id: &str) -> Result<Group, StorageError> {
        self.labels().load_group(label_id)
    }
    /// Loads all visibility labels (decrypted).
    ///
    /// Labels are sorted by decrypted name in Rust since encrypted names
    /// cannot be sorted in SQL (#128).
    pub fn load_all_groups(&self) -> Result<Vec<Group>, StorageError> {
        self.labels().load_all_groups()
    }
    /// Deletes a visibility label.
    pub fn delete_group(&self, label_id: &str) -> Result<(), StorageError> {
        self.labels().delete_group(label_id)
    }
    /// Saves a per-contact visibility override.
    pub fn save_contact_override(
        &self,
        contact_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> Result<(), StorageError> {
        self.labels()
            .save_contact_override(contact_id, field_id, is_visible)
    }
    /// Deletes a per-contact visibility override.
    pub fn delete_contact_override(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> Result<(), StorageError> {
        self.labels().delete_contact_override(contact_id, field_id)
    }
    /// Loads all per-contact overrides for a contact.
    pub fn load_contact_overrides(
        &self,
        contact_id: &str,
    ) -> Result<HashMap<String, bool>, StorageError> {
        self.labels().load_contact_overrides(contact_id)
    }
    /// Loads all per-contact overrides (all contacts).
    pub fn load_all_contact_overrides(
        &self,
    ) -> Result<HashMap<String, HashMap<String, bool>>, StorageError> {
        self.labels().load_all_contact_overrides()
    }
    /// Deletes all per-contact overrides for a contact.
    pub fn delete_all_contact_overrides(&self, contact_id: &str) -> Result<(), StorageError> {
        self.labels().delete_all_contact_overrides(contact_id)
    }
    /// Saves a complete GroupManager to storage.
    ///
    /// This saves all groups and all per-contact overrides.
    pub fn save_group_manager(&self, manager: &GroupManager) -> Result<(), StorageError> {
        self.labels().save_group_manager(manager)
    }
    /// Loads a complete GroupManager from storage.
    ///
    /// This loads all groups and all per-contact overrides.
    pub fn load_group_manager(&self) -> Result<GroupManager, StorageError> {
        self.labels().load_group_manager()
    }
    /// Creates a label in storage.
    ///
    /// Returns the created label.
    pub fn create_group(&self, name: &str) -> Result<Group, StorageError> {
        self.labels().create_group(name)
    }
    /// Renames a label in storage.
    pub fn rename_group(&self, label_id: &str, new_name: &str) -> Result<(), StorageError> {
        self.labels().rename_group(label_id, new_name)
    }
    /// Adds a contact to a label in storage.
    pub fn add_contact_to_group(
        &self,
        label_id: &str,
        contact_id: &str,
    ) -> Result<(), StorageError> {
        self.labels().add_contact_to_group(label_id, contact_id)
    }
    /// Removes a contact from a label in storage.
    pub fn remove_contact_from_group(
        &self,
        label_id: &str,
        contact_id: &str,
    ) -> Result<(), StorageError> {
        self.labels()
            .remove_contact_from_group(label_id, contact_id)
    }
    /// Removes a contact from all labels in storage.
    ///
    /// Call this when deleting a contact.
    pub fn remove_contact_from_all_groups(&self, contact_id: &str) -> Result<(), StorageError> {
        self.labels().remove_contact_from_all_groups(contact_id)
    }
    /// Sets a field's visibility for a label in storage.
    pub fn set_group_field_visibility(
        &self,
        label_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> Result<(), StorageError> {
        self.labels()
            .set_group_field_visibility(label_id, field_id, is_visible)
    }
    /// Gets all labels that contain a specific contact.
    pub fn get_groups_for_contact(&self, contact_id: &str) -> Result<Vec<Group>, StorageError> {
        self.labels().get_groups_for_contact(contact_id)
    }
}
