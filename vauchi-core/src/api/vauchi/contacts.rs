// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact and double ratchet operations.

use crate::contact::Contact;
use crate::contact::display::{AvatarPreference, ContactDisplayOptions, DisplayNamePreference};
use crate::contact_card::normalize_avatar;
use crate::crypto::SymmetricKey;
use crate::crypto::ratchet::DoubleRatchetState;

use super::super::contact_manager::ContactManager;
use super::super::error::{VauchiError, VauchiResult};
use super::builder::decoy_id_to_fake_pk;
use super::{AuthMode, Vauchi};

impl Vauchi {
    // === Contact Operations ===

    /// Converts decoy contact tuples into `Contact` objects (display-only).
    fn decoy_contacts_as_contacts(&self) -> VauchiResult<Vec<Contact>> {
        let decoys = self.storage.load_decoy_contacts()?;
        Ok(decoys
            .into_iter()
            .map(|(id, _display_name, card)| {
                Contact::from_exchange(decoy_id_to_fake_pk(&id), card, SymmetricKey::generate())
            })
            .collect())
    }

    /// Gets a contact by ID.
    ///
    /// In duress mode, only decoy contacts are accessible.
    pub fn get_contact(&self, id: &str) -> VauchiResult<Option<Contact>> {
        if self.auth_mode == AuthMode::Duress {
            return Ok(self
                .decoy_contacts_as_contacts()?
                .into_iter()
                .find(|c| c.id() == id));
        }
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.get_contact(id)
    }

    /// Lists all contacts, respecting the current auth mode.
    ///
    /// - **Normal** or **Unauthenticated**: Returns real contacts (filtered
    ///   by hidden status, as before).
    /// - **Duress**: Returns decoy contacts only, presented as real contacts.
    pub fn list_contacts(&self) -> VauchiResult<Vec<Contact>> {
        if self.auth_mode == AuthMode::Duress {
            return self.decoy_contacts_as_contacts();
        }
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.list_contacts()
    }

    /// Lists contacts with pagination.
    ///
    /// In duress mode, paginates over decoy contacts only.
    pub fn list_contacts_paginated(
        &self,
        offset: usize,
        limit: usize,
    ) -> VauchiResult<Vec<Contact>> {
        if self.auth_mode == AuthMode::Duress {
            let all = self.decoy_contacts_as_contacts()?;
            return Ok(all.into_iter().skip(offset).take(limit).collect());
        }
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.list_contacts_paginated(offset, limit)
    }

    /// Searches contacts by display name.
    ///
    /// In duress mode, searches decoy contacts only.
    pub fn search_contacts(&self, query: &str) -> VauchiResult<Vec<Contact>> {
        if self.auth_mode == AuthMode::Duress {
            let query_lower = query.to_lowercase();
            return Ok(self
                .decoy_contacts_as_contacts()?
                .into_iter()
                .filter(|c| c.display_name().to_lowercase().contains(&query_lower))
                .collect());
        }
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.search_contacts(query)
    }

    /// Finds contacts by fuzzy matching on display name or ID prefix.
    ///
    /// In duress mode, fuzzy-matches decoy contacts only.
    pub fn find_contact_fuzzy(&self, query: &str) -> VauchiResult<Vec<Contact>> {
        if self.auth_mode == AuthMode::Duress {
            let query_lower = query.to_lowercase();
            return Ok(self
                .decoy_contacts_as_contacts()?
                .into_iter()
                .filter(|c| {
                    c.display_name().to_lowercase().contains(&query_lower)
                        || c.id().starts_with(query)
                })
                .collect());
        }
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
    ///
    /// In duress mode, returns the decoy contact count.
    pub fn contact_count(&self) -> VauchiResult<usize> {
        if self.auth_mode == AuthMode::Duress {
            return Ok(self.storage.load_decoy_contacts()?.len());
        }
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.contact_count()
    }

    /// Accepts an incoming exchange from the relay.
    ///
    /// Performs X3DH key agreement, creates the contact, and
    /// initializes the Double Ratchet — all within core.
    /// Frontends call this instead of doing X3DH directly.
    ///
    /// Returns the created contact's ID.
    pub fn accept_relay_exchange(
        &self,
        identity_key: &[u8; 32],
        ephemeral_key: &[u8; 32],
        display_name: &str,
    ) -> VauchiResult<String> {
        use crate::exchange::{X3DH, X3DHKeyPair};

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        // Check if contact already exists
        let public_id = hex::encode(identity_key);
        if self.storage.load_contact(&public_id)?.is_some() {
            return Err(VauchiError::Configuration(format!(
                "Contact {} already exists",
                public_id
            )));
        }

        // X3DH key agreement as responder
        let our_x3dh = identity.x3dh_keypair();
        let shared_secret = X3DH::respond(&our_x3dh, identity_key, ephemeral_key).map_err(|e| {
            VauchiError::Exchange(crate::exchange::ExchangeError::KeyAgreementFailed(format!(
                "X3DH failed: {:?}",
                e
            )))
        })?;

        // Create contact
        let card = crate::ContactCard::new(display_name);
        let contact = Contact::from_exchange(*identity_key, card, shared_secret.clone());
        let contact_id = contact.id().to_string();

        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.add_contact(contact)?;

        // Initialize Double Ratchet
        let ratchet_dh = X3DHKeyPair::from_bytes(*our_x3dh.secret_bytes());
        self.create_ratchet_as_responder(&contact_id, &shared_secret, ratchet_dh)?;

        Ok(contact_id)
    }

    /// Accepts an encrypted relay exchange message (ADR-021).
    ///
    /// Decrypts the EncryptedExchangeMessage, extracts identity and display
    /// name, creates the contact, and initializes the Double Ratchet.
    /// All crypto stays in core — frontends pass opaque bytes.
    ///
    /// Returns (contact_id, sender_exchange_key) — the exchange key is needed
    /// by the frontend to send an encrypted response via relay.
    pub fn accept_encrypted_relay_exchange(
        &self,
        message_bytes: &[u8],
    ) -> VauchiResult<(String, [u8; 32])> {
        use crate::exchange::EncryptedExchangeMessage;

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let encrypted_msg = EncryptedExchangeMessage::from_bytes(message_bytes)
            .map_err(|e| VauchiError::Serialization(format!("exchange message: {:?}", e)))?;

        let our_x3dh = identity.x3dh_keypair();
        let (payload, _shared_secret) = encrypted_msg
            .decrypt(&our_x3dh)
            .map_err(|e| VauchiError::Crypto(format!("exchange decrypt: {:?}", e)))?;

        let exchange_key = payload.exchange_key;
        let contact_id = self.accept_relay_exchange(
            &payload.identity_key,
            &exchange_key,
            &payload.display_name,
        )?;

        Ok((contact_id, exchange_key))
    }

    /// Creates an encrypted exchange response message (ADR-021).
    ///
    /// Encrypts our identity key and display name for the recipient.
    /// Frontends call this to get opaque bytes for relay transport.
    pub fn create_encrypted_exchange_response(
        &self,
        recipient_exchange_key: &[u8; 32],
    ) -> VauchiResult<Vec<u8>> {
        use crate::exchange::EncryptedExchangeMessage;

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let our_x3dh = identity.x3dh_keypair();
        let (encrypted_msg, _) = EncryptedExchangeMessage::create(
            &our_x3dh,
            recipient_exchange_key,
            identity.signing_public_key(),
            identity.display_name(),
        )
        .map_err(|e| VauchiError::Crypto(format!("exchange encrypt: {:?}", e)))?;

        encrypted_msg
            .to_bytes()
            .map_err(|e| VauchiError::Serialization(format!("exchange serialize: {:?}", e)))
    }

    /// Adds a new contact from an exchange.
    pub fn add_contact(&self, contact: Contact) -> VauchiResult<()> {
        // Record sync before add (need contact data while we still own it)
        if contact.is_exchanged() {
            let sync_data = crate::sync::ContactSyncData::from_contact(&contact);
            self.record_sync_item(crate::sync::SyncItem::ContactAdded {
                contact_data: sync_data,
                timestamp: self.now_timestamp(),
            });
        }
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.add_contact(contact)
    }

    /// Removes a contact by ID.
    pub fn remove_contact(&self, id: &str) -> VauchiResult<bool> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        let removed = manager.remove_contact(id)?;
        if removed {
            self.record_sync_item(crate::sync::SyncItem::ContactRemoved {
                contact_id: id.to_string(),
                timestamp: self.now_timestamp(),
            });
        }
        Ok(removed)
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
            contact
                .trust_for_recovery()
                .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        } else {
            contact
                .untrust_for_recovery()
                .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        }

        self.storage.save_contact(&contact)?;

        // Upload updated guardian entries to relay after trust change
        #[cfg(feature = "network-http")]
        {
            // Best-effort: log but don't fail the local toggle on relay errors
            if let Err(_e) = self.upload_guardian_entries() {
                // Relay upload failed — local state is correct, relay will be
                // updated on next sync or manual retry. Silent for now;
                // future: emit an event for the UI to show a warning.
            }
        }

        Ok(new_state)
    }

    /// Verifies a contact's fingerprint.
    pub fn verify_contact_fingerprint(&self, id: &str) -> VauchiResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.verify_fingerprint(id)
    }

    /// Removes fingerprint verification from a contact.
    pub fn unverify_contact_fingerprint(&self, id: &str) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .load_contact(id)?
            .ok_or_else(|| VauchiError::NotFound(format!("contact: {}", id)))?;
        contact
            .mark_fingerprint_unverified()
            .map_err(|e| VauchiError::InvalidState(format!("unverify fingerprint: {}", e)))?;
        self.storage.save_contact(&contact)?;
        Ok(())
    }

    // === Soft-Delete / Archive Operations ===

    /// Soft-deletes an imported contact.
    pub fn soft_delete_imported_contact(&self, id: &str) -> VauchiResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.soft_delete_imported_contact(id)
    }

    /// Undoes a soft-delete on an imported contact.
    pub fn undo_delete_imported_contact(&self, id: &str) -> VauchiResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.undo_delete_imported_contact(id)
    }

    /// Permanently deletes an imported contact from storage.
    pub fn hard_delete_imported_contact(&self, id: &str) -> VauchiResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.hard_delete_imported_contact(id)?;
        self.record_sync_item(crate::sync::SyncItem::ContactRemoved {
            contact_id: id.to_string(),
            timestamp: self.now_timestamp(),
        });
        Ok(())
    }

    /// Archives an exchanged contact.
    pub fn archive_contact(&self, id: &str) -> VauchiResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.archive_contact(id)?;
        self.record_sync_item(crate::sync::SyncItem::ContactArchived {
            contact_id: id.to_string(),
            timestamp: self.now_timestamp(),
        });
        Ok(())
    }

    /// Unarchives an exchanged contact.
    pub fn unarchive_contact(&self, id: &str) -> VauchiResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.unarchive_contact(id)?;
        self.record_sync_item(crate::sync::SyncItem::ContactUnarchived {
            contact_id: id.to_string(),
            timestamp: self.now_timestamp(),
        });
        Ok(())
    }

    /// Lists all archived contacts.
    pub fn list_archived_contacts(&self) -> VauchiResult<Vec<Contact>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.list_archived_contacts()
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

    /// Adds or replaces a personal note for a contact.
    ///
    /// Encrypts the plaintext note using the contact's shared key.
    /// Frontends MUST use this instead of calling crypto::encrypt directly.
    pub fn add_personal_note(&self, contact_id: &str, note_text: &str) -> VauchiResult<()> {
        use crate::crypto::encrypt;

        let contact = self
            .storage
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(contact_id.to_string()))?;
        let shared_key = contact
            .shared_key()
            .ok_or_else(|| VauchiError::Configuration("Contact has no shared key".into()))?;
        let encrypted = encrypt(shared_key, note_text.as_bytes())
            .map_err(|e| VauchiError::Configuration(format!("Encryption failed: {}", e)))?;
        self.storage.save_personal_notes(contact_id, &encrypted)?;
        Ok(())
    }

    /// Reads the personal note for a contact, decrypting it.
    ///
    /// Returns None if no note exists.
    pub fn read_personal_note(&self, contact_id: &str) -> VauchiResult<Option<String>> {
        use crate::crypto::decrypt;

        let encrypted = match self.storage.load_personal_notes(contact_id)? {
            Some(data) => data,
            None => return Ok(None),
        };
        let contact = self
            .storage
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(contact_id.to_string()))?;
        let shared_key = contact
            .shared_key()
            .ok_or_else(|| VauchiError::Configuration("Contact has no shared key".into()))?;
        let plaintext = decrypt(shared_key, &encrypted)
            .map_err(|e| VauchiError::Configuration(format!("Decryption failed: {}", e)))?;
        Ok(Some(String::from_utf8(plaintext).map_err(|e| {
            VauchiError::Configuration(format!("Note is not valid UTF-8: {}", e))
        })?))
    }

    /// Saves encrypted personal notes for a contact (raw bytes).
    ///
    /// Low-level API for sync/migration. Prefer `add_personal_note()`.
    pub fn save_personal_notes(
        &self,
        contact_id: &str,
        notes_encrypted: &[u8],
    ) -> VauchiResult<()> {
        self.storage
            .save_personal_notes(contact_id, notes_encrypted)?;
        Ok(())
    }

    /// Loads encrypted personal notes for a contact (raw bytes).
    ///
    /// Low-level API for sync/migration. Prefer `read_personal_note()`.
    pub fn load_personal_notes(&self, contact_id: &str) -> VauchiResult<Option<Vec<u8>>> {
        Ok(self.storage.load_personal_notes(contact_id)?)
    }

    /// Deletes personal notes for a contact.
    pub fn delete_personal_notes(&self, contact_id: &str) -> VauchiResult<()> {
        self.storage.delete_personal_notes(contact_id)?;
        Ok(())
    }

    // === Contact Nickname Operations ===

    /// Sets a local nickname for a contact. Validates: non-empty after trim, <= 100 chars.
    pub fn set_contact_nickname(&self, contact_id: &str, nickname: &str) -> VauchiResult<()> {
        let trimmed = nickname.trim();
        if trimmed.is_empty() {
            return Err(VauchiError::InvalidState("Nickname cannot be empty".into()));
        }
        if trimmed.chars().count() > 100 {
            return Err(VauchiError::InvalidState(
                "Nickname exceeds 100 characters".into(),
            ));
        }
        self.storage
            .save_contact_nickname(contact_id, trimmed.as_bytes())?;
        Ok(())
    }

    /// Clears the local nickname for a contact.
    ///
    /// Resets display name preference to Primary if it was Custom.
    pub fn clear_contact_nickname(&self, contact_id: &str) -> VauchiResult<()> {
        self.storage.delete_contact_nickname(contact_id)?;
        let (name_pref, _) = self.storage.load_display_preferences(contact_id)?;
        if name_pref == DisplayNamePreference::Custom {
            self.storage
                .save_display_name_preference(contact_id, &DisplayNamePreference::Primary)?;
        }
        Ok(())
    }

    /// Returns the local nickname for a contact, or None if unset.
    pub fn get_contact_nickname(&self, contact_id: &str) -> VauchiResult<Option<String>> {
        Ok(self.storage.load_contact_nickname(contact_id)?)
    }

    // === Contact Custom Avatar Operations ===

    /// Sets a custom avatar for a contact.
    ///
    /// Accepts any supported image format (PNG, JPEG, BMP, WebP).
    /// Core normalizes to WebP <= 32 KB internally (ADR-042).
    pub fn set_contact_custom_avatar(&self, contact_id: &str, data: &[u8]) -> VauchiResult<()> {
        let webp = normalize_avatar(data).map_err(|e: crate::contact_card::ContactCardError| {
            VauchiError::InvalidState(e.to_string())
        })?;
        self.storage.save_contact_custom_avatar(contact_id, &webp)?;
        Ok(())
    }

    /// Clears the custom avatar for a contact.
    ///
    /// Resets avatar preference to Primary if it was Custom.
    pub fn clear_contact_custom_avatar(&self, contact_id: &str) -> VauchiResult<()> {
        self.storage.delete_contact_custom_avatar(contact_id)?;
        let (_, avatar_pref) = self.storage.load_display_preferences(contact_id)?;
        if avatar_pref == AvatarPreference::Custom {
            self.storage
                .save_avatar_preference(contact_id, &AvatarPreference::Primary)?;
        }
        Ok(())
    }

    /// Returns the custom avatar for a contact, or None if unset.
    pub fn get_contact_custom_avatar(&self, contact_id: &str) -> VauchiResult<Option<Vec<u8>>> {
        Ok(self.storage.load_contact_custom_avatar(contact_id)?)
    }

    // === Shared Name Operations (internal, called by sync layer) ===

    /// Adds a shared name for a contact from an inbound sync delta.
    pub fn add_contact_shared_name(
        &self,
        contact_id: &str,
        name: &str,
        is_primary: bool,
    ) -> VauchiResult<()> {
        self.storage.add_shared_name(contact_id, name, is_primary)?;
        Ok(())
    }

    /// Removes a shared name for a contact from an inbound sync delta.
    pub fn remove_contact_shared_name(&self, contact_id: &str, name: &str) -> VauchiResult<()> {
        self.storage.remove_shared_name(contact_id, name)?;
        Ok(())
    }

    /// Lists all shared names for a contact.
    pub fn list_contact_shared_names(
        &self,
        contact_id: &str,
    ) -> VauchiResult<Vec<crate::contact::display::SharedName>> {
        Ok(self.storage.list_shared_names(contact_id)?)
    }

    /// Adds a shared avatar for a contact from an inbound sync delta.
    pub fn add_contact_shared_avatar(
        &self,
        contact_id: &str,
        avatar_data: &[u8],
        is_primary: bool,
    ) -> VauchiResult<()> {
        use sha2::{Digest, Sha256};
        let hash = hex::encode(Sha256::digest(avatar_data));
        self.storage
            .add_shared_avatar(contact_id, &hash, avatar_data, is_primary)?;
        Ok(())
    }

    /// Removes a shared avatar for a contact from an inbound sync delta.
    pub fn remove_contact_shared_avatar(
        &self,
        contact_id: &str,
        avatar_hash: &str,
    ) -> VauchiResult<()> {
        self.storage.remove_shared_avatar(contact_id, avatar_hash)?;
        Ok(())
    }

    /// Lists all shared avatars for a contact.
    pub fn list_contact_shared_avatars(
        &self,
        contact_id: &str,
    ) -> VauchiResult<Vec<crate::contact::display::SharedAvatar>> {
        Ok(self.storage.list_shared_avatars(contact_id)?)
    }

    // === Display Preference Operations ===

    /// Sets the display name preference for a contact.
    ///
    /// Setting `Custom` when no nickname is set returns `InvalidState`.
    /// Setting `SharedName` when name not in shared set returns `InvalidState`.
    pub fn set_display_name_preference(
        &self,
        contact_id: &str,
        pref: DisplayNamePreference,
    ) -> VauchiResult<()> {
        match &pref {
            DisplayNamePreference::Primary => {}
            DisplayNamePreference::Custom => {
                let nick = self.storage.load_contact_nickname(contact_id)?;
                if nick.is_none() {
                    return Err(VauchiError::InvalidState(
                        "Cannot set Custom name preference without a nickname".into(),
                    ));
                }
            }
            DisplayNamePreference::SharedName { name } => {
                let names = self.storage.list_shared_names(contact_id)?;
                if !names.iter().any(|n| n.name == *name) {
                    return Err(VauchiError::InvalidState(format!(
                        "Shared name '{}' not found",
                        name
                    )));
                }
            }
        }
        self.storage
            .save_display_name_preference(contact_id, &pref)?;
        Ok(())
    }

    /// Sets the avatar preference for a contact.
    ///
    /// Setting `Custom` when no custom avatar is set returns `InvalidState`.
    /// Setting `SharedAvatar` when hash not in shared set returns `InvalidState`.
    pub fn set_avatar_preference(
        &self,
        contact_id: &str,
        pref: AvatarPreference,
    ) -> VauchiResult<()> {
        match &pref {
            AvatarPreference::Primary => {}
            AvatarPreference::Custom => {
                let has = self.storage.has_contact_custom_avatar(contact_id)?;
                if !has {
                    return Err(VauchiError::InvalidState(
                        "Cannot set Custom avatar preference without a custom avatar".into(),
                    ));
                }
            }
            AvatarPreference::SharedAvatar { hash } => {
                let avatars = self.storage.list_shared_avatars(contact_id)?;
                if !avatars.iter().any(|a| a.avatar_hash == *hash) {
                    return Err(VauchiError::InvalidState(format!(
                        "Shared avatar '{}' not found",
                        hash
                    )));
                }
            }
        }
        self.storage.save_avatar_preference(contact_id, &pref)?;
        Ok(())
    }

    /// Returns all display options for a contact (for the chooser screen).
    pub fn get_contact_display_options(
        &self,
        contact_id: &str,
    ) -> VauchiResult<ContactDisplayOptions> {
        use crate::contact::display::*;

        let _contact = self
            .storage
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(contact_id.to_string()))?;
        let nickname = self.storage.load_contact_nickname(contact_id)?;
        let shared_names = self.storage.list_shared_names(contact_id)?;
        let shared_avatars = self.storage.list_shared_avatars(contact_id)?;
        let (name_pref, avatar_pref) = self.storage.load_display_preferences(contact_id)?;
        let has_custom_avatar = self.storage.has_contact_custom_avatar(contact_id)?;

        // Build name options: shared names + custom nickname
        let mut names: Vec<NameOption> = shared_names
            .iter()
            .map(|n| NameOption {
                source: if n.is_primary {
                    DisplayNamePreference::Primary
                } else {
                    DisplayNamePreference::SharedName {
                        name: n.name.clone(),
                    }
                },
                name: n.name.clone(),
                is_primary: n.is_primary,
            })
            .collect();
        if let Some(ref nick) = nickname {
            names.push(NameOption {
                source: DisplayNamePreference::Custom,
                name: nick.clone(),
                is_primary: false,
            });
        }

        // Build avatar options: shared avatars + custom avatar
        let mut avatars: Vec<AvatarOption> = shared_avatars
            .iter()
            .map(|a| AvatarOption {
                source: if a.is_primary {
                    AvatarPreference::Primary
                } else {
                    AvatarPreference::SharedAvatar {
                        hash: a.avatar_hash.clone(),
                    }
                },
                has_data: true,
                is_primary: a.is_primary,
            })
            .collect();
        avatars.push(AvatarOption {
            source: AvatarPreference::Custom,
            has_data: has_custom_avatar,
            is_primary: false,
        });

        Ok(ContactDisplayOptions {
            names,
            avatars,
            active_name_preference: name_pref,
            active_avatar_preference: avatar_pref,
        })
    }

    // === Imported Contact Editing ===

    /// Update a field value on an imported contact.
    ///
    /// Imported contacts allow full local editing (the user owns the card).
    /// Exchanged contacts reject this — their card is owned by the other party.
    pub fn update_imported_contact_field(
        &self,
        id: &str,
        field_id: &str,
        new_value: &str,
    ) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        if contact.is_exchanged() {
            return Err(VauchiError::InvalidState(
                "Cannot edit exchanged contact fields — card is owned by the other party".into(),
            ));
        }
        let mut card = contact.card().clone();
        card.update_field_value(field_id, new_value)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        contact.update_card(card);
        self.storage.save_contact(&contact)?;
        Ok(())
    }

    /// Add a field to an imported contact.
    ///
    /// Exchanged contacts reject this — their card is owned by the other party.
    pub fn add_imported_contact_field(
        &self,
        id: &str,
        field_type: crate::contact_card::FieldType,
        label: &str,
        value: &str,
    ) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        if contact.is_exchanged() {
            return Err(VauchiError::InvalidState(
                "Cannot edit exchanged contact fields — card is owned by the other party".into(),
            ));
        }
        let mut card = contact.card().clone();
        card.add_field(crate::contact_card::ContactField::new(
            field_type, label, value,
        ))
        .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        contact.update_card(card);
        self.storage.save_contact(&contact)?;
        Ok(())
    }

    /// Remove a field from an imported contact.
    ///
    /// Exchanged contacts reject this — their card is owned by the other party.
    pub fn remove_imported_contact_field(&self, id: &str, field_id: &str) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        if contact.is_exchanged() {
            return Err(VauchiError::InvalidState(
                "Cannot edit exchanged contact fields — card is owned by the other party".into(),
            ));
        }
        let mut card = contact.card().clone();
        card.remove_field(field_id)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        contact.update_card(card);
        self.storage.save_contact(&contact)?;
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

    /// Cleans up stale soft-deleted contacts (older than 30 seconds).
    ///
    /// Should be called on startup to hard-delete contacts whose undo window
    /// expired while the app was not running (e.g., crash during undo window).
    pub fn cleanup_stale_soft_deletes(&self) -> VauchiResult<usize> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let threshold = now.saturating_sub(30);
        let stale_ids = self.storage.find_stale_soft_deletes(threshold)?;
        let count = stale_ids.len();
        for id in stale_ids {
            self.storage.delete_contact(id.as_str())?;
        }
        Ok(count)
    }

    /// Expire pending reciprocity confirmations older than 7 days.
    ///
    /// Called at startup / session-init to persist the passive timer's
    /// read-time `Pending → Unreciprocated` transition into storage.
    /// Returns the number of contacts transitioned.
    pub fn expire_pending_reciprocity(&self) -> VauchiResult<usize> {
        use crate::exchange::reciprocity::Reciprocity;

        let pending = self.storage.list_contacts_by_reciprocity("pending")?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut count = 0;
        for mut contact in pending {
            let ts = contact.exchange_timestamp().unwrap_or(0);
            if now > ts + 7 * 24 * 60 * 60 {
                contact.set_reciprocity(Reciprocity::Unreciprocated);
                self.storage.save_contact(&contact)?;
                count += 1;
            }
        }
        Ok(count)
    }
}
