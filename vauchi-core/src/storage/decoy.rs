// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`DecoyStore`](super::DecoyStore).

use super::{Storage, StorageError};
use crate::contact_card::ContactCard;

impl Storage {
    /// Saves a decoy contact.
    ///
    /// The card is encrypted with the storage key before persisting.
    /// Uses INSERT OR REPLACE for idempotent saves.
    pub fn save_decoy_contact(
        &self,
        id: &str,
        display_name: &str,
        card: &ContactCard,
    ) -> Result<(), StorageError> {
        self.decoy().save_decoy_contact(id, display_name, card)
    }
    /// Loads all decoy contacts.
    ///
    /// Returns a list of (id, display_name, card) tuples.
    pub fn load_decoy_contacts(&self) -> Result<Vec<(String, String, ContactCard)>, StorageError> {
        self.decoy().load_decoy_contacts()
    }
    /// Deletes a single decoy contact by ID.
    pub fn delete_decoy_contact(&self, id: &str) -> Result<(), StorageError> {
        self.decoy().delete_decoy_contact(id)
    }
    /// Deletes all decoy contacts.
    pub fn clear_all_decoy_contacts(&self) -> Result<(), StorageError> {
        self.decoy().clear_all_decoy_contacts()
    }
}
