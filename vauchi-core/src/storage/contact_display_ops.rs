// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact-display forwarders to [`ContactStore`](super::ContactStore).

use super::{Storage, StorageError};
use crate::contact::display::{AvatarPreference, DisplayNamePreference, SharedAvatar, SharedName};
use std::collections::{HashMap, HashSet};

impl Storage {
    /// Saves an encrypted nickname for a contact.
    ///
    /// The caller passes plaintext bytes; this method encrypts with the storage
    /// encryption key before writing to the `nickname_encrypted` column.
    pub fn save_contact_nickname(
        &self,
        contact_id: &str,
        nickname_bytes: &[u8],
    ) -> Result<(), StorageError> {
        self.contacts()
            .save_contact_nickname(contact_id, nickname_bytes)
    }
    /// Loads and decrypts the nickname for a contact.
    ///
    /// Returns `None` if no nickname is stored, or `NotFound` if the contact
    /// does not exist.
    pub fn load_contact_nickname(&self, contact_id: &str) -> Result<Option<String>, StorageError> {
        self.contacts().load_contact_nickname(contact_id)
    }
    /// Clears the nickname for a contact by setting the column to NULL.
    pub fn delete_contact_nickname(&self, contact_id: &str) -> Result<(), StorageError> {
        self.contacts().delete_contact_nickname(contact_id)
    }
    /// Saves an encrypted custom avatar for a contact.
    pub fn save_contact_custom_avatar(
        &self,
        contact_id: &str,
        avatar_bytes: &[u8],
    ) -> Result<(), StorageError> {
        self.contacts()
            .save_contact_custom_avatar(contact_id, avatar_bytes)
    }
    /// Loads and decrypts the custom avatar for a contact.
    pub fn load_contact_custom_avatar(
        &self,
        contact_id: &str,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        self.contacts().load_contact_custom_avatar(contact_id)
    }
    /// Clears the custom avatar for a contact.
    pub fn delete_contact_custom_avatar(&self, contact_id: &str) -> Result<(), StorageError> {
        self.contacts().delete_contact_custom_avatar(contact_id)
    }
    /// Checks if a contact has a custom avatar without loading/decrypting it.
    pub fn has_contact_custom_avatar(&self, contact_id: &str) -> Result<bool, StorageError> {
        self.contacts().has_contact_custom_avatar(contact_id)
    }
    //
    // Encrypted with the storage key (self.encryption_key), not the contact's shared key.
    // Shared names are local-only data — the flat set received from the sender.

    /// Adds or updates a shared name for a contact.
    ///
    /// When `is_primary` is true, clears the previous primary first
    /// to maintain the exactly-one-primary invariant.
    pub fn add_shared_name(
        &self,
        contact_id: &str,
        name: &str,
        is_primary: bool,
    ) -> Result<(), StorageError> {
        self.contacts()
            .add_shared_name(contact_id, name, is_primary)
    }
    /// Removes a shared name for a contact.
    pub fn remove_shared_name(&self, contact_id: &str, name: &str) -> Result<(), StorageError> {
        self.contacts().remove_shared_name(contact_id, name)
    }
    /// Lists all shared names for a contact (primary first).
    pub fn list_shared_names(&self, contact_id: &str) -> Result<Vec<SharedName>, StorageError> {
        self.contacts().list_shared_names(contact_id)
    }
    /// Adds or updates a shared avatar for a contact.
    ///
    /// When `is_primary` is true, clears the previous primary first
    /// to maintain the exactly-one-primary invariant.
    pub fn add_shared_avatar(
        &self,
        contact_id: &str,
        avatar_hash: &str,
        avatar_data: &[u8],
        is_primary: bool,
    ) -> Result<(), StorageError> {
        self.contacts()
            .add_shared_avatar(contact_id, avatar_hash, avatar_data, is_primary)
    }
    /// Removes a shared avatar for a contact.
    pub fn remove_shared_avatar(
        &self,
        contact_id: &str,
        avatar_hash: &str,
    ) -> Result<(), StorageError> {
        self.contacts()
            .remove_shared_avatar(contact_id, avatar_hash)
    }
    /// Lists all shared avatars for a contact (primary first).
    pub fn list_shared_avatars(&self, contact_id: &str) -> Result<Vec<SharedAvatar>, StorageError> {
        self.contacts().list_shared_avatars(contact_id)
    }
    /// Saves the display name preference for a contact.
    pub fn save_display_name_preference(
        &self,
        contact_id: &str,
        pref: &DisplayNamePreference,
    ) -> Result<(), StorageError> {
        self.contacts()
            .save_display_name_preference(contact_id, pref)
    }
    /// Saves the avatar preference for a contact.
    pub fn save_avatar_preference(
        &self,
        contact_id: &str,
        pref: &AvatarPreference,
    ) -> Result<(), StorageError> {
        self.contacts().save_avatar_preference(contact_id, pref)
    }
    /// Loads both display preferences for a contact.
    pub fn load_display_preferences(
        &self,
        contact_id: &str,
    ) -> Result<(DisplayNamePreference, AvatarPreference), StorageError> {
        self.contacts().load_display_preferences(contact_id)
    }
    // === Batch Operations (N+1 prevention) ===

    /// Batch-load shared names for multiple contacts in a single query.
    ///
    /// Returns a map of contact_id → Vec<SharedName>. Contacts with no shared
    /// names are absent from the map (callers should use `.unwrap_or_default()`).
    pub fn batch_shared_names(
        &self,
        contact_ids: &[&str],
    ) -> Result<HashMap<String, Vec<SharedName>>, StorageError> {
        self.contacts().batch_shared_names(contact_ids)
    }
    /// Batch-load nicknames for multiple contacts in a single query.
    ///
    /// Returns a map of contact_id → decrypted nickname string. Contacts with
    /// no nickname (NULL) are absent from the map.
    pub fn batch_nicknames(
        &self,
        contact_ids: &[&str],
    ) -> Result<HashMap<String, String>, StorageError> {
        self.contacts().batch_nicknames(contact_ids)
    }
    /// Batch-load display preferences for multiple contacts in a single query.
    ///
    /// Returns a map of contact_id → (DisplayNamePreference, AvatarPreference).
    /// Contacts absent from the result should fall back to their defaults.
    pub fn batch_display_preferences(
        &self,
        contact_ids: &[&str],
    ) -> Result<HashMap<String, (DisplayNamePreference, AvatarPreference)>, StorageError> {
        self.contacts().batch_display_preferences(contact_ids)
    }
    /// Batch-check which contacts have custom avatars.
    ///
    /// Returns the set of contact_ids that have a non-NULL custom avatar.
    /// Absence from the set means no custom avatar.
    pub fn batch_has_custom_avatar(
        &self,
        contact_ids: &[&str],
    ) -> Result<HashSet<String>, StorageError> {
        self.contacts().batch_has_custom_avatar(contact_ids)
    }
}
