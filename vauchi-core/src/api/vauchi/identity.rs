// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identity and contact card operations.

use std::sync::Arc;

use crate::contact_card::{ContactCard, ContactField};
use crate::identity::Identity;
use crate::network::Transport;
use crate::storage::SecureStorage;

use super::super::contact_manager::ContactManager;
use super::super::error::{VauchiError, VauchiResult};
use super::{Vauchi, SMK_KEY_NAME};

impl<T: Transport> Vauchi<T> {
    // === Identity Operations ===

    /// Sets the SecureStorage backend for SMK persistence.
    ///
    /// Call this before `create_identity()` to enable SMK-based encryption,
    /// or before `migrate_to_smk()` for upgrading existing installations.
    pub fn set_secure_storage(&mut self, secure_storage: Arc<dyn SecureStorage>) {
        self.secure_storage = Some(secure_storage);
    }

    /// Returns a reference to the SecureStorage, if set.
    pub fn secure_storage(&self) -> Option<&dyn SecureStorage> {
        self.secure_storage.as_deref()
    }

    /// Creates a new identity with the given display name.
    ///
    /// If SecureStorage is set, derives SMK from the identity's master seed,
    /// stores it in SecureStorage, and re-encrypts storage with the SMK-derived SEK.
    pub fn create_identity(&mut self, display_name: &str) -> VauchiResult<()> {
        if self.identity.is_some() {
            return Err(VauchiError::AlreadyInitialized);
        }

        let identity = Identity::create(display_name);

        // Create initial contact card from identity
        let card = ContactCard::new(display_name);
        self.storage.save_own_card(&card)?;

        // If SecureStorage is available, derive and store SMK, then rekey storage
        if let Some(ref ss) = self.secure_storage {
            let smk = identity.derive_smk();

            // Store SMK in SecureStorage BEFORE rekey (safety: see DP-1 rationale)
            ss.save_key(SMK_KEY_NAME, smk.as_bytes())
                .map_err(|e| VauchiError::Configuration(format!("Failed to store SMK: {}", e)))?;

            // Derive SEK and rekey storage
            let sek = smk.derive_sek();
            self.storage.rekey(sek).map_err(|e| {
                VauchiError::Configuration(format!("Failed to rekey storage: {}", e))
            })?;
        }

        // Persist identity to storage so it survives restart
        self.storage
            .save_identity(&identity.to_storage_bytes(), identity.display_name())?;

        self.identity = Some(identity);
        Ok(())
    }

    /// Migrates an existing installation from old storage_key to SMK-derived SEK.
    ///
    /// Requires:
    /// - SecureStorage is set (`set_secure_storage()` called)
    /// - Identity is loaded (via `set_identity()` or `load_identity()`)
    /// - Storage is open with the old key
    ///
    /// Flow (see Phase 2a.3):
    /// 1. Derive SMK from identity's master_seed
    /// 2. Store SMK in SecureStorage (before rekey for safety)
    /// 3. Derive SEK from SMK
    /// 4. Rekey all encrypted columns to SEK
    pub fn migrate_to_smk(&mut self) -> VauchiResult<()> {
        let ss = self
            .secure_storage
            .as_ref()
            .ok_or_else(|| VauchiError::Configuration("SecureStorage not set".into()))?;

        // Check if already migrated
        if ss
            .has_key(SMK_KEY_NAME)
            .map_err(|e| VauchiError::Configuration(format!("Failed to check SMK: {}", e)))?
        {
            return Ok(()); // Already migrated
        }

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let smk = identity.derive_smk();

        // Store SMK in SecureStorage BEFORE rekey
        ss.save_key(SMK_KEY_NAME, smk.as_bytes())
            .map_err(|e| VauchiError::Configuration(format!("Failed to store SMK: {}", e)))?;

        // Derive SEK and rekey storage
        let sek = smk.derive_sek();
        self.storage
            .rekey(sek)
            .map_err(|e| VauchiError::Configuration(format!("Failed to rekey storage: {}", e)))?;

        Ok(())
    }

    /// Sets an existing identity.
    pub fn set_identity(&mut self, identity: Identity) -> VauchiResult<()> {
        if self.identity.is_some() {
            return Err(VauchiError::AlreadyInitialized);
        }
        self.identity = Some(identity);
        Ok(())
    }

    /// Exports the identity as an encrypted backup.
    ///
    /// Returns the backup data as a hex-encoded string.
    pub fn export_backup(&self, password: &str) -> VauchiResult<String> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let backup = identity
            .export_backup(password)
            .map_err(|e| VauchiError::Configuration(format!("Export failed: {:?}", e)))?;
        Ok(hex::encode(backup.as_bytes()))
    }

    /// Imports an identity from an encrypted backup.
    ///
    /// The backup_data should be a hex-encoded string from `export_backup`.
    pub fn import_backup(&mut self, backup_data: &str, password: &str) -> VauchiResult<()> {
        let bytes = hex::decode(backup_data.trim())
            .map_err(|e| VauchiError::Configuration(format!("Invalid hex data: {}", e)))?;
        let backup = crate::identity::IdentityBackup::new(bytes.clone());
        let identity = Identity::import_backup(&backup, password)
            .map_err(|e| VauchiError::Configuration(format!("Import failed: {:?}", e)))?;

        let name = identity.display_name().to_string();

        // Persist to storage
        self.storage.save_identity(&bytes, &name)?;

        // Create contact card if none exists
        if self.storage.load_own_card()?.is_none() {
            let card = crate::contact_card::ContactCard::new(&name);
            self.storage.save_own_card(&card)?;
        }

        self.identity = Some(identity);
        Ok(())
    }

    /// Returns the current identity, if set.
    pub fn identity(&self) -> Option<&Identity> {
        self.identity.as_ref()
    }

    /// Returns the public ID of the current identity.
    pub fn public_id(&self) -> VauchiResult<String> {
        self.identity
            .as_ref()
            .map(|id| id.public_id())
            .ok_or(VauchiError::IdentityNotInitialized)
    }

    /// Returns the formatted fingerprint of the current identity's public key.
    ///
    /// The fingerprint is the hex-encoded public key formatted as 16 groups
    /// of 4 uppercase hex characters (e.g., "ABCD 1234 EF56 ...").
    pub fn own_fingerprint(&self) -> VauchiResult<String> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let hex = hex::encode(identity.signing_public_key());
        Ok(hex
            .chars()
            .collect::<Vec<_>>()
            .chunks(4)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(" ")
            .to_uppercase())
    }

    /// Returns true if an identity has been created or set.
    pub fn has_identity(&self) -> bool {
        self.identity.is_some()
    }

    /// Updates the user's display name.
    ///
    /// Updates both the identity and contact card display name.
    /// Returns an error if:
    /// - No identity is set
    /// - The name is empty or whitespace-only
    /// - The name exceeds 100 characters
    pub fn update_display_name(&mut self, new_name: &str) -> VauchiResult<()> {
        let name = new_name.trim();

        if name.is_empty() {
            return Err(VauchiError::InvalidState(
                "Display name cannot be empty".into(),
            ));
        }
        if name.len() > 100 {
            return Err(VauchiError::InvalidState(
                "Display name cannot exceed 100 characters".into(),
            ));
        }

        // Get mutable reference to identity
        let identity = self
            .identity
            .as_mut()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        // Update identity display name
        identity.set_display_name(name);

        // Update contact card display name
        let mut card = self
            .storage
            .load_own_card()?
            .unwrap_or_else(|| ContactCard::new(name));
        card.set_display_name(name)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        self.storage.save_own_card(&card)?;

        Ok(())
    }

    // === Contact Card Operations ===

    /// Gets the user's own contact card.
    pub fn own_card(&self) -> VauchiResult<Option<ContactCard>> {
        Ok(self.storage.load_own_card()?)
    }

    /// Updates the user's own contact card.
    pub fn update_own_card(&self, card: &ContactCard) -> VauchiResult<Vec<String>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.update_own_card(card)
    }

    /// Adds a field to the user's own card.
    pub fn add_own_field(&self, field: ContactField) -> VauchiResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.add_field_to_own_card(field)
    }

    /// Removes a field from the user's own card by label.
    pub fn remove_own_field(&self, label: &str) -> VauchiResult<bool> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.remove_field_from_own_card(label)
    }

    /// Removes a field from the user's own card by field ID.
    pub fn remove_own_field_by_id(&self, field_id: &str) -> VauchiResult<bool> {
        let card = self
            .storage
            .load_own_card()?
            .ok_or_else(|| VauchiError::InvalidState("No own card found".into()))?;
        let label = card
            .fields()
            .iter()
            .find(|f| f.id() == field_id)
            .map(|f| f.label().to_string());
        let Some(label) = label else {
            return Ok(false);
        };
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.remove_field_from_own_card(&label)
    }

    /// Sets whether a field is shown in no-group visibility mode.
    ///
    /// When no groups exist, this controls field visibility directly.
    /// Persists the updated card to storage.
    pub fn set_field_shown(&self, field_id: &str, shown: bool) -> VauchiResult<()> {
        let mut card = self
            .storage
            .load_own_card()?
            .ok_or_else(|| VauchiError::InvalidState("No own card found".into()))?;
        card.set_field_shown(field_id, shown);
        self.storage.save_own_card(&card)?;
        Ok(())
    }
}
