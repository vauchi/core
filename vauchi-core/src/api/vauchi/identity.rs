// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identity and contact card operations.

use std::sync::Arc;

use crate::contact_card::{ContactCard, ContactField};
use crate::identity::Identity;
use crate::types::BackupReminderState;

use crate::storage::SecureStorage;

use super::super::contact_manager::ContactManager;
use super::super::error::{VauchiError, VauchiResult};
use super::super::events::VauchiEvent;
use super::{SMK_KEY_NAME, Vauchi};

impl Vauchi {
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
    #[tracing::instrument(level = "info", skip_all, name = "vauchi.create_identity")]
    pub fn create_identity(&mut self, display_name: &str) -> VauchiResult<()> {
        if self.identity.is_some() {
            return Err(VauchiError::AlreadyInitialized);
        }

        let identity = Identity::create(display_name);

        // Create initial contact card, or update display name on existing card.
        // During onboarding, fields may already be saved to the card before
        // identity creation — preserve them by loading the existing card.
        let card = match self.storage.load_own_card()? {
            Some(mut existing) => {
                let _ = existing.set_display_name(display_name);
                existing
            }
            None => ContactCard::new(display_name),
        };
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

    /// Creates an identity and atomically marks onboarding as
    /// complete in a single FFI call.
    ///
    /// This closes the crash window that exists when frontends
    /// orchestrate the two writes themselves
    /// (`createIdentity → setOnboardingCompleted`) — a process
    /// kill between the two left the next launch in a state where
    /// identity exists but onboarding is "incomplete", and the
    /// next launch's onboarding flow would attempt a second
    /// `create_identity` (which fails loudly with
    /// `AlreadyInitialized`, but the user sees a confusing screen).
    ///
    /// On error from the second step (the storage write of
    /// `mark_onboarding_complete`), the identity stays created but
    /// onboarding stays incomplete — the next launch resumes
    /// onboarding from the existing identity rather than asking
    /// the user to create a duplicate. See audit
    /// `2026-04-28-app-launch-and-identity-orchestration-in-core`
    /// §2.5.
    pub fn create_identity_with_onboarding(&mut self, display_name: &str) -> VauchiResult<()> {
        self.create_identity(display_name)?;
        self.mark_onboarding_complete()?;
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
        let _ = self.record_backup_completed();
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

    /// Exports a full v3 backup (identity + contacts + own card + labels).
    ///
    /// Returns the backup data as a hex-encoded string.
    pub fn export_full_backup(&self, password: &str) -> VauchiResult<String> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let identity_data = crate::backup::FullBackupIdentityData {
            display_name: identity.display_name().to_string(),
            master_seed: *identity.master_seed(),
            device_index: identity.device_index(),
            device_name: identity.device_info().device_name().to_string(),
        };

        let contacts = self.storage.list_contacts()?;
        let own_card = self.storage.load_own_card()?;
        let groups = self.storage.load_all_groups()?;
        let labels: Vec<(String, String, Vec<String>)> = groups
            .iter()
            .map(|g| {
                (
                    g.id().to_string(),
                    g.name().to_string(),
                    g.contacts().iter().cloned().collect(),
                )
            })
            .collect();

        let blob = crate::backup::export_full_backup(
            &identity_data,
            &contacts,
            own_card.as_ref(),
            &labels,
            password,
        )
        .map_err(|e| VauchiError::Configuration(format!("Full backup export failed: {e}")))?;

        let _ = self.record_backup_completed();
        Ok(hex::encode(blob))
    }

    /// Imports a full v3 backup, restoring identity, contacts, own card, and labels.
    ///
    /// The backup_data should be a hex-encoded string from `export_full_backup`.
    /// Fails if an identity is already set (restore onto a fresh instance only).
    pub fn import_full_backup(&mut self, backup_data: &str, password: &str) -> VauchiResult<()> {
        if self.identity.is_some() {
            return Err(VauchiError::AlreadyInitialized);
        }

        let bytes = hex::decode(backup_data.trim())
            .map_err(|e| VauchiError::Configuration(format!("Invalid hex data: {e}")))?;

        let envelope = crate::backup::import_full_backup(&bytes, password)
            .map_err(|e| VauchiError::Configuration(format!("Full backup import failed: {e}")))?;

        // Restore identity from envelope
        let seed = crate::backup::extract_master_seed(&envelope.sections.identity)
            .map_err(|e| VauchiError::Configuration(format!("Seed extraction failed: {e}")))?;

        let identity = Identity::from_device_link(
            *seed,
            envelope.sections.identity.display_name.clone(),
            envelope.sections.identity.device_index,
            envelope.sections.identity.device_name.clone(),
        );

        // Persist identity (v2 format for storage compatibility)
        let id_backup = identity
            .export_backup(password)
            .map_err(|e| VauchiError::Configuration(format!("Identity re-export failed: {e:?}")))?;
        self.storage
            .save_identity(id_backup.as_bytes(), identity.display_name())?;

        // Restore own card
        if let Some(card) = &envelope.sections.own_card {
            self.storage.save_own_card(card)?;
        } else {
            let card = crate::contact_card::ContactCard::new(identity.display_name());
            self.storage.save_own_card(&card)?;
        }

        // Restore contacts
        let contacts = crate::backup::restore_contacts_from_envelope(&envelope)
            .map_err(|e| VauchiError::Configuration(format!("Contact restore failed: {e}")))?;
        for contact in &contacts {
            self.storage.save_contact(contact)?;
        }

        // Restore labels
        for label in &envelope.sections.labels {
            let contacts: std::collections::HashSet<String> =
                label.contacts.iter().cloned().collect();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("Time went backwards")
                .as_secs();
            let group = crate::contact::Group::from_storage(
                label.label_id.clone(),
                label.name.clone(),
                contacts,
                std::collections::HashSet::new(),
                None,
                now,
                now,
            );
            self.storage.save_group(&group)?;
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
    ///
    /// If the in-memory `identity` is `None`, falls back to checking
    /// storage. This handles the case where another `Vauchi` instance
    /// pointing at the same database (e.g. `PlatformAppEngine`'s
    /// internal Vauchi vs. `VauchiPlatform`'s Vauchi on Android) wrote
    /// an identity to disk after this instance was constructed —
    /// without the storage check we would incorrectly return `false`
    /// for the lifetime of the process, which left mobile bottom-tab
    /// nav empty after onboarding (only the Onboarding tab was
    /// returned by `tab_info`).
    pub fn has_identity(&self) -> bool {
        if self.identity.is_some() {
            return true;
        }
        matches!(self.storage.load_identity(), Ok(Some(_)))
    }

    /// Re-load identity from storage into `self.identity` if currently None.
    ///
    /// Companion to [`Vauchi::has_identity`]'s storage-fallback. Use this
    /// before code paths that read `self.identity` directly (e.g. screen
    /// builders inside the AppEngine, signing/encryption flows). Without
    /// it, those paths would still see `None` after a sibling Vauchi
    /// instance — pointing at the same DB — wrote an identity to storage,
    /// because the in-memory cache wasn't refreshed.
    ///
    /// Idempotent: returns immediately if `self.identity` is already
    /// populated.
    pub fn refresh_identity_from_storage(&mut self) {
        if self.identity.is_some() {
            return;
        }
        if let Ok(Some((bytes, _display_name))) = self.storage.load_identity()
            && let Ok(identity) = Identity::from_storage_bytes(&bytes)
        {
            self.identity = Some(identity);
        }
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
        let old_name = card.display_name().to_string();
        card.set_display_name(name)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        self.storage.save_own_card(&card)?;

        if old_name != name {
            self.events.dispatch(VauchiEvent::OwnCardUpdated {
                changed_fields: vec!["display_name".into()],
            });
        }

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
        let changed_labels = manager.update_own_card(card)?;
        let ts = self.now_timestamp();
        for label in &changed_labels {
            // Look up the current value for the changed field
            let value = card
                .fields()
                .iter()
                .find(|f| f.label() == label)
                .map(|f| f.value().to_string())
                .unwrap_or_default();
            self.record_sync_item(crate::sync::SyncItem::CardUpdated {
                field_label: label.clone(),
                new_value: value,
                timestamp: ts,
            });
        }
        Ok(changed_labels)
    }

    /// Adds a field to the user's own card.
    pub fn add_own_field(&self, field: ContactField) -> VauchiResult<()> {
        let label = field.label().to_string();
        let value = field.value().to_string();
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.add_field_to_own_card(field)?;
        self.record_sync_item(crate::sync::SyncItem::CardUpdated {
            field_label: label,
            new_value: value,
            timestamp: self.now_timestamp(),
        });
        Ok(())
    }

    /// Removes a field from the user's own card by label.
    pub fn remove_own_field(&self, label: &str) -> VauchiResult<bool> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        let removed = manager.remove_field_from_own_card(label)?;
        if removed {
            self.record_sync_item(crate::sync::SyncItem::CardUpdated {
                field_label: label.to_string(),
                new_value: String::new(), // empty = removal
                timestamp: self.now_timestamp(),
            });
        }
        Ok(removed)
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

    // === Backup Reminder Operations ===

    /// Loads backup reminder state, returning defaults if none persisted.
    pub fn load_backup_reminder_state(&self) -> VauchiResult<BackupReminderState> {
        self.storage
            .load_backup_reminder_state()
            .map(|opt| opt.unwrap_or_default())
            .map_err(Into::into)
    }

    /// Saves backup reminder state to encrypted storage.
    pub fn save_backup_reminder_state(&self, state: &BackupReminderState) -> VauchiResult<()> {
        self.storage
            .save_backup_reminder_state(state)
            .map_err(Into::into)
    }

    /// Records that a backup completed successfully (resets reminder count).
    pub fn record_backup_completed(&self) -> VauchiResult<()> {
        let mut state = self.load_backup_reminder_state()?;
        state.record_backup();
        self.save_backup_reminder_state(&state)
    }

    // === App Preferences (theme + language) ===
    //
    // Singleton row in the `app_preferences` table (migration V46).
    // Device-local — preferences do not sync. The Settings screen
    // dropdown writes through `set_app_preferences`; the engine
    // populates `SettingsConfig` from `app_preferences()`. See
    // problem record `2026-05-01-android-humble-ui-deep-retirement`.

    /// Loads app preferences (theme + language). Returns the default
    /// (`follow_system_*` both `true`, both ids `None`) if no row has
    /// been written yet.
    pub fn app_preferences(&self) -> VauchiResult<crate::types::AppPreferences> {
        self.storage.load_app_preferences().map_err(Into::into)
    }

    /// Saves app preferences (theme + language) to the singleton row.
    pub fn set_app_preferences(&self, prefs: &crate::types::AppPreferences) -> VauchiResult<()> {
        self.storage.save_app_preferences(prefs).map_err(Into::into)
    }
}
