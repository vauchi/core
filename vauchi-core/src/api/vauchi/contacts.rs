// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact and double ratchet operations.

use crate::contact::Contact;
use crate::crypto::SymmetricKey;
use crate::crypto::ratchet::DoubleRatchetState;

use super::super::contact_manager::ContactManager;
use super::super::error::{VauchiError, VauchiResult};
use super::builder::decoy_id_to_fake_pk;
use super::{AuthMode, Vauchi};

impl Vauchi {
    // === Contact Operations ===

    /// Gets a contact by ID.
    pub fn get_contact(&self, id: &str) -> VauchiResult<Option<Contact>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.get_contact(id)
    }

    /// Lists all contacts, respecting the current auth mode.
    ///
    /// - **Normal** or **Unauthenticated**: Returns real contacts (filtered
    ///   by hidden status, as before).
    /// - **Duress**: Returns decoy contacts only, presented as real contacts.
    pub fn list_contacts(&self) -> VauchiResult<Vec<Contact>> {
        match self.auth_mode {
            AuthMode::Duress => {
                // Load decoy contacts and convert to Contact structs
                let decoys = self.storage.load_decoy_contacts()?;
                Ok(decoys
                    .into_iter()
                    .map(|(id, _display_name, card)| {
                        Contact::from_exchange(
                            // Use a deterministic "public key" derived from the ID
                            // (decoys don't have real keys — this is display-only)
                            decoy_id_to_fake_pk(&id),
                            card,
                            crate::crypto::SymmetricKey::generate(),
                        )
                    })
                    .collect())
            }
            AuthMode::Normal | AuthMode::Unauthenticated => {
                let manager = ContactManager::new(&self.storage, self.events.clone());
                manager.list_contacts()
            }
        }
    }

    /// Lists contacts with pagination.
    pub fn list_contacts_paginated(
        &self,
        offset: usize,
        limit: usize,
    ) -> VauchiResult<Vec<Contact>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.list_contacts_paginated(offset, limit)
    }

    /// Searches contacts by display name.
    pub fn search_contacts(&self, query: &str) -> VauchiResult<Vec<Contact>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.search_contacts(query)
    }

    /// Finds contacts by fuzzy matching on display name or ID prefix.
    ///
    /// Combines case-insensitive name substring matching with ID prefix matching.
    /// Returns the union of both result sets, deduplicated.
    pub fn find_contact_fuzzy(&self, query: &str) -> VauchiResult<Vec<Contact>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.find_contact_fuzzy(query)
    }

    /// Finds a visibility label by fuzzy matching on name or ID prefix.
    ///
    /// First tries case-insensitive name matching, then ID prefix matching.
    /// Returns the first match, or `None` if no label matches.
    pub fn find_group_fuzzy(&self, query: &str) -> VauchiResult<Option<crate::contact::Group>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.find_group_fuzzy(query)
    }

    /// Checks if a contact's sender has been revoked (they deleted their identity).
    pub fn is_contact_revoked(&self, contact_id: &str) -> bool {
        self.storage.is_sender_revoked(contact_id).unwrap_or(false)
    }

    /// Returns the number of contacts.
    pub fn contact_count(&self) -> VauchiResult<usize> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.contact_count()
    }

    /// Adds a new contact from an exchange.
    pub fn add_contact(&self, contact: Contact) -> VauchiResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.add_contact(contact)
    }

    /// Removes a contact by ID.
    pub fn remove_contact(&self, id: &str) -> VauchiResult<bool> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.remove_contact(id)
    }

    /// Updates an existing contact.
    pub fn update_contact(&self, contact: &Contact) -> VauchiResult<()> {
        self.storage.save_contact(contact)?;
        Ok(())
    }

    /// Returns the contact limit.
    pub fn get_contact_limit(&self) -> VauchiResult<usize> {
        Ok(self.storage.get_contact_limit()?)
    }

    /// Toggles recovery trust for a contact.
    ///
    /// Returns the new trust state (true = now trusted, false = now untrusted).
    pub fn toggle_recovery_trust(&self, contact_id: &str) -> VauchiResult<bool> {
        let mut contact = self
            .storage
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::InvalidState("Contact not found".into()))?;

        if contact.is_blocked() {
            return Err(VauchiError::InvalidState(
                "Blocked contacts cannot be trusted for recovery".into(),
            ));
        }

        let new_state = !contact.is_recovery_trusted();
        if new_state {
            contact.trust_for_recovery();
        } else {
            contact.untrust_for_recovery();
        }

        self.storage.save_contact(&contact)?;
        Ok(new_state)
    }

    /// Verifies a contact's fingerprint.
    pub fn verify_contact_fingerprint(&self, id: &str) -> VauchiResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.verify_fingerprint(id)
    }

    // === Double Ratchet Operations ===

    /// Gets the Double Ratchet state for a contact.
    pub fn get_ratchet_state(&self, contact_id: &str) -> VauchiResult<Option<DoubleRatchetState>> {
        Ok(self.storage.load_ratchet_state(contact_id)?.map(|(r, _)| r))
    }

    /// Saves a Double Ratchet state for a contact.
    ///
    /// If a ratchet state already exists, preserves the is_initiator flag.
    pub fn save_ratchet_state(
        &self,
        contact_id: &str,
        state: &DoubleRatchetState,
    ) -> VauchiResult<()> {
        // Load existing to preserve is_initiator flag
        let is_initiator = self
            .storage
            .load_ratchet_state(contact_id)?
            .map(|(_, i)| i)
            .unwrap_or(true);
        self.storage
            .save_ratchet_state(contact_id, state, is_initiator)?;
        Ok(())
    }

    /// Creates and saves a new ratchet state for a contact as initiator.
    pub fn create_ratchet_as_initiator(
        &self,
        contact_id: &str,
        shared_secret: &SymmetricKey,
        their_dh_public: [u8; 32],
    ) -> VauchiResult<()> {
        let ratchet = DoubleRatchetState::initialize_initiator(shared_secret, their_dh_public)
            .map_err(|e| crate::api::VauchiError::Crypto(e.to_string()))?;
        self.storage
            .save_ratchet_state(contact_id, &ratchet, true)?;
        Ok(())
    }

    /// Creates and saves a new ratchet state for a contact as responder.
    pub fn create_ratchet_as_responder(
        &self,
        contact_id: &str,
        shared_secret: &SymmetricKey,
        our_dh: crate::exchange::X3DHKeyPair,
    ) -> VauchiResult<()> {
        let ratchet = DoubleRatchetState::initialize_responder(shared_secret, our_dh);
        self.storage
            .save_ratchet_state(contact_id, &ratchet, false)?;
        Ok(())
    }

    // === Personal Notes Operations ===

    /// Saves encrypted personal notes for a contact.
    pub fn save_personal_notes(
        &self,
        contact_id: &str,
        notes_encrypted: &[u8],
    ) -> VauchiResult<()> {
        self.storage
            .save_personal_notes(contact_id, notes_encrypted)?;
        Ok(())
    }

    /// Loads encrypted personal notes for a contact.
    pub fn load_personal_notes(&self, contact_id: &str) -> VauchiResult<Option<Vec<u8>>> {
        Ok(self.storage.load_personal_notes(contact_id)?)
    }

    /// Deletes personal notes for a contact.
    pub fn delete_personal_notes(&self, contact_id: &str) -> VauchiResult<()> {
        self.storage.delete_personal_notes(contact_id)?;
        Ok(())
    }

    // === Contact Field Notes Operations ===

    /// Saves encrypted per-field notes for a contact.
    pub fn save_contact_field_note(
        &self,
        contact_id: &str,
        field_id: &str,
        note_encrypted: &[u8],
    ) -> VauchiResult<()> {
        self.storage
            .save_contact_field_note(contact_id, field_id, note_encrypted)?;
        Ok(())
    }

    /// Loads all encrypted per-field notes for a contact.
    ///
    /// Returns a `HashMap<field_id, note_encrypted>`. Returns an empty map if
    /// the contact has no field notes.
    pub fn load_contact_field_notes(
        &self,
        contact_id: &str,
    ) -> VauchiResult<std::collections::HashMap<String, Vec<u8>>> {
        Ok(self.storage.load_contact_field_notes(contact_id)?)
    }

    /// Deletes the encrypted note for a specific `(contact_id, field_id)` pair.
    pub fn delete_contact_field_note(&self, contact_id: &str, field_id: &str) -> VauchiResult<()> {
        self.storage
            .delete_contact_field_note(contact_id, field_id)?;
        Ok(())
    }
}
