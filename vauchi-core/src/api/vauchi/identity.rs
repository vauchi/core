// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identity and contact card operations.

use std::sync::Arc;

use crate::contact_card::{ContactCard, ContactField};
use crate::identity::Identity;
use crate::types::BackupReminderState;
use crate::types::SettingsFlags;

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

        let identity = Identity::create(display_name, self.clock.unix_seconds());

        // Create initial contact card, or update display name on existing card.
        // During onboarding, fields may already be saved to the card before
        // identity creation — preserve them by loading the existing card.
        let card = match self.storage.contacts().load_own_card()? {
            Some(mut existing) => {
                // Propagate validation errors so the user sees why their
                // display name was rejected instead of seeing the old name
                // silently retained.
                existing
                    .set_display_name(display_name)
                    .map_err(VauchiError::from)?;
                existing
            }
            None => ContactCard::new(display_name),
        };
        self.storage.contacts().save_own_card(&card)?;

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
            .identity()
            .save_identity(&identity.to_storage_bytes(), identity.display_name())?;

        // Fresh installs never need the field-centric grandfathering sweep —
        // every field they ever add starts under the hidden-by-default model
        // (2026-07-05-ungrouped-contacts-default-open).
        let mut flags = self.load_settings_flags()?;
        flags.field_centric_visibility_migrated = true;
        self.save_settings_flags(&flags)?;

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
        // best-effort: backup-completed timestamp is a UX hint for the
        // reminder system; export already succeeded above so the user-
        // visible action is complete
        #[allow(clippy::let_underscore_must_use)]
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
        let identity = Identity::import_backup(&backup, password, self.clock.unix_seconds())
            .map_err(|e| VauchiError::Configuration(format!("Import failed: {:?}", e)))?;

        let name = identity.display_name().to_string();

        // Persist in the same plaintext storage format as
        // `create_identity` — the DB encrypts at rest (ADR-015).
        // Persisting the password-encrypted backup bytes locked the
        // user out on restart: no startup loader has the backup
        // password (2026-06-11-restore-identity-unloadable-after-restart).
        self.storage
            .identity()
            .save_identity(&identity.to_storage_bytes(), &name)?;

        // Create contact card if none exists
        if self.storage.contacts().load_own_card()?.is_none() {
            let card = crate::contact_card::ContactCard::new(&name);
            self.storage.contacts().save_own_card(&card)?;
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

        let contacts = self.storage.contacts().list_contacts()?;
        let own_card = self.storage.contacts().load_own_card()?;
        let groups = self.storage.labels().load_all_groups()?;
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
            self.clock.unix_seconds(),
        )
        .map_err(|e| VauchiError::Configuration(format!("Full backup export failed: {e}")))?;

        // best-effort: same as export_backup — timestamp is a UX hint
        #[allow(clippy::let_underscore_must_use)]
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
            self.clock.unix_seconds(),
        );

        // Persist in the same plaintext storage format as
        // `create_identity` — the DB encrypts at rest (ADR-015).
        // Re-exporting with the backup password both locked the user
        // out on restart (no startup loader has that password) and
        // re-ran password-strength validation at restore time
        // (2026-06-11-restore-identity-unloadable-after-restart).
        self.storage
            .identity()
            .save_identity(&identity.to_storage_bytes(), identity.display_name())?;

        // Restore own card
        if let Some(card) = &envelope.sections.own_card {
            self.storage.contacts().save_own_card(card)?;
        } else {
            let card = crate::contact_card::ContactCard::new(identity.display_name());
            self.storage.contacts().save_own_card(&card)?;
        }

        // Restore contacts
        let contacts = crate::backup::restore_contacts_from_envelope(&envelope)
            .map_err(|e| VauchiError::Configuration(format!("Contact restore failed: {e}")))?;
        for contact in &contacts {
            self.storage.contacts().save_contact(contact)?;
        }

        // Restore labels
        for label in &envelope.sections.labels {
            let contacts: std::collections::HashSet<String> =
                label.contacts.iter().cloned().collect();
            let now = self.clock.unix_seconds();
            let group = crate::contact::Group::from_storage(
                label.label_id.clone(),
                label.name.clone(),
                contacts,
                std::collections::HashSet::new(),
                // The backup label section carries no presentation overrides
                // (display name / bio / avatar) — restore them empty.
                None,
                None,
                None,
                now,
                now,
            );
            self.storage.labels().save_group(&group)?;
        }

        self.identity = Some(identity);
        Ok(())
    }

    /// Adopt a decrypted device-link response onto this fresh instance,
    /// becoming a new device of an existing identity (M5 B3 join engine).
    ///
    /// Mirrors [`Self::import_full_backup`]: fresh-instance guard, build
    /// the identity from the shared master seed, persist it + the device
    /// registry, then apply the full sync payload (own card, contacts,
    /// tags, …) if the linking device sent one. The caller (the join
    /// responder machine) owns the ephemeral responder that decrypted the
    /// response, so this takes the already-decrypted
    /// [`crate::exchange::DeviceLinkResponse`], not the QR + ciphertext.
    pub fn adopt_device_link_response(
        &mut self,
        response: &crate::exchange::DeviceLinkResponse,
        device_name: String,
    ) -> VauchiResult<()> {
        if self.identity.is_some() {
            return Err(VauchiError::AlreadyInitialized);
        }

        let now = self.clock.unix_seconds();
        let device_index = response.device_index();
        let identity = Identity::from_device_link(
            *response.master_seed(),
            response.display_name().to_string(),
            device_index,
            device_name.clone(),
            now,
        );

        // Same plaintext storage format as `create_identity`; the DB
        // encrypts at rest (ADR-015).
        self.storage
            .identity()
            .save_identity(&identity.to_storage_bytes(), identity.display_name())?;

        // Persist the registry from the linking device so sync can address
        // the peer devices.
        self.storage
            .device()
            .save_device_registry(response.registry())?;

        if response.sync_payload_json().is_empty() {
            // No sync payload — seed a default own card from the display name.
            let card = ContactCard::new(identity.display_name());
            self.storage.contacts().save_own_card(&card)?;
        } else {
            let payload: crate::sync::DeviceSyncPayload =
                serde_json::from_str(response.sync_payload_json()).map_err(|e| {
                    VauchiError::Configuration(format!(
                        "Device-link sync payload parse failed: {e}"
                    ))
                })?;
            // DeviceInfo isn't Clone; reconstruct this device's info with the
            // same derivation `from_device_link` used above (deterministic).
            let current_device = crate::identity::DeviceInfo::derive(
                response.master_seed(),
                device_index,
                device_name,
                now,
            );
            let mut orchestrator = crate::api::sync::DeviceSyncOrchestrator::new(
                &self.storage,
                current_device,
                response.registry().clone(),
            );
            orchestrator.apply_full_sync(payload).map_err(|e| {
                VauchiError::Configuration(format!("Device-link sync apply failed: {e}"))
            })?;
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
        matches!(self.storage.identity().load_identity(), Ok(Some(_)))
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
        if let Ok(Some((bytes, _display_name))) = self.storage.identity().load_identity()
            && let Ok(identity) = Identity::from_storage_bytes(&bytes, self.clock.unix_seconds())
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
            .contacts()
            .load_own_card()?
            .unwrap_or_else(|| ContactCard::new(name));
        let old_name = card.display_name().to_string();
        card.set_display_name(name)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        self.storage.contacts().save_own_card(&card)?;

        if old_name != name {
            self.events.dispatch(VauchiEvent::OwnCardUpdated {
                changed_fields: vec!["display_name".into()],
            });
            // A renamed card must repropagate to contacts — the display name is
            // shared with every contact (mirrors add/remove/update_own_field).
            // The per-contact last_sent_display_name baseline makes the
            // repropagate emit a DisplayNameChanged exactly when the name
            // changed (2026-06-29-card-update-duplicate-message-paths).
            self.mark_own_card_repropagate()?;
        }

        Ok(())
    }

    // === Contact Card Operations ===

    /// Gets the user's own contact card.
    pub fn own_card(&self) -> VauchiResult<Option<ContactCard>> {
        Ok(self.storage.contacts().load_own_card()?)
    }

    /// Updates the user's own contact card.
    pub fn update_own_card(&self, card: &ContactCard) -> VauchiResult<Vec<String>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        let changed_labels = manager.update_own_card(card)?;
        if !changed_labels.is_empty() {
            self.mark_own_card_repropagate()?;
        }
        let ts = self.now_timestamp();
        for label in &changed_labels {
            match card.fields().iter().find(|field| field.label() == label) {
                Some(field) => self.record_sync_item(crate::sync::SyncItem::CardFieldSynced {
                    field: field.clone(),
                    field_visibility: card
                        .field_visibility()
                        .contains(field.id())
                        .then(|| card.field_visibility().get(field.id()).clone()),
                    timestamp: ts,
                }),
                None => self.record_sync_item(crate::sync::SyncItem::CardFieldRemoved {
                    field_label: label.clone(),
                    timestamp: ts,
                }),
            }
        }
        Ok(changed_labels)
    }

    /// Adds a field to the user's own card. New entries default to hidden;
    /// the `new_field_default_visible` setting materializes an explicit
    /// `Everyone` toggle at add time instead (never a lazy read-time
    /// fallback, so "unruled" stays unambiguous — Decision 2,
    /// 2026-07-05-ungrouped-contacts-default-open).
    pub fn add_own_field(&self, field: ContactField) -> VauchiResult<()> {
        let field_id = field.id().to_string();
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.add_field_to_own_card(field)?;
        if self.load_settings_flags()?.new_field_default_visible {
            self.set_own_field_public(&field_id)?;
        }
        self.mark_own_card_repropagate()?;
        let card = self
            .storage
            .contacts()
            .load_own_card()?
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let synced_field = card
            .fields()
            .iter()
            .find(|current| current.id() == field_id)
            .cloned()
            .ok_or_else(|| VauchiError::InvalidState("own field was not saved".into()))?;
        let field_visibility = card
            .field_visibility()
            .contains(&field_id)
            .then(|| card.field_visibility().get(&field_id).clone());
        self.record_sync_item(crate::sync::SyncItem::CardFieldSynced {
            field: synced_field,
            field_visibility,
            timestamp: self.now_timestamp(),
        });
        Ok(())
    }

    /// Removes a field from the user's own card by label.
    pub fn remove_own_field(&self, label: &str) -> VauchiResult<bool> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        let removed = manager.remove_field_from_own_card(label)?;
        if removed {
            self.mark_own_card_repropagate()?;
            self.record_sync_item(crate::sync::SyncItem::CardFieldRemoved {
                field_label: label.to_string(),
                timestamp: self.now_timestamp(),
            });
        }
        Ok(removed)
    }

    /// Removes a field from the user's own card by field ID.
    pub fn remove_own_field_by_id(&self, field_id: &str) -> VauchiResult<bool> {
        let card = self
            .storage
            .contacts()
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
        // Route through the by-label path so the removal also arms the
        // repropagation marker and records the device-sync item (the manager
        // call alone does neither).
        self.remove_own_field(&label)
    }

    /// Marks the own card dirty so the next sync tick repropagates it to
    /// contacts (group-aware, via `run_owed_repropagation`). A fresh edit
    /// resets the retry budget so a previously backed-off marker resumes.
    ///
    /// Public so a frontend that edits the own card directly (rather than via
    /// the `*_own_field` API) can arm the same retry path.
    pub fn mark_own_card_repropagate(&self) -> VauchiResult<()> {
        self.storage
            .ux()
            .save_own_card_repropagate(&crate::types::OwnCardRepropagateState {
                needs_repropagate: true,
                failed_attempts: 0,
            })?;
        Ok(())
    }

    /// Sets an unassigned entry's Visible/Hidden toggle (explicit
    /// `Everyone`/`Nobody`). The toggle governs what every contact sees
    /// (field-centric model), so the change arms repropagation.
    pub fn set_field_shown(&self, field_id: &str, shown: bool) -> VauchiResult<()> {
        let mut card = self
            .storage
            .contacts()
            .load_own_card()?
            .ok_or_else(|| VauchiError::InvalidState("No own card found".into()))?;
        card.set_field_shown(field_id, shown);
        self.storage.contacts().save_own_card(&card)?;
        self.mark_own_card_repropagate()?;
        Ok(())
    }

    // === Backup Reminder Operations ===

    /// Loads backup reminder state, returning defaults if none persisted.
    pub fn load_backup_reminder_state(&self) -> VauchiResult<BackupReminderState> {
        self.storage
            .ux()
            .load_backup_reminder_state()
            .map(|opt| opt.unwrap_or_default())
            .map_err(Into::into)
    }

    /// Loads persisted settings flags, returning defaults if none persisted.
    pub fn load_settings_flags(&self) -> VauchiResult<SettingsFlags> {
        self.storage
            .ux()
            .load_settings_flags()
            .map(|opt| opt.unwrap_or_default())
            .map_err(Into::into)
    }

    /// Saves settings flags to encrypted storage.
    pub fn save_settings_flags(&self, flags: &SettingsFlags) -> VauchiResult<()> {
        self.storage
            .ux()
            .save_settings_flags(flags)
            .map_err(Into::into)
    }

    /// Saves backup reminder state to encrypted storage.
    pub fn save_backup_reminder_state(&self, state: &BackupReminderState) -> VauchiResult<()> {
        self.storage
            .ux()
            .save_backup_reminder_state(state)
            .map_err(Into::into)
    }

    /// Records that a backup completed successfully (resets reminder count).
    pub fn record_backup_completed(&self) -> VauchiResult<()> {
        let mut state = self.load_backup_reminder_state()?;
        state.record_backup(self.clock.unix_seconds());
        self.save_backup_reminder_state(&state)
    }

    // === Guardian Key Shard Backup Operations ===

    /// Exports a full v4 guardian backup and splits the encryption key into shards.
    ///
    /// Returns the encrypted backup blob (hex-encoded) and a vector of sealed
    /// shares, one per guardian. The backup key is generated randomly, split
    /// with Shamir's Secret Sharing, and each share is sealed to the guardian's
    /// X25519 public key (derived from their Ed25519 identity key).
    ///
    /// **Share ordering:** `sealed_shares[i]` corresponds to `guardian_pks[i]`.
    /// The caller must preserve this mapping when distributing shares to
    /// guardians; otherwise recovery will reconstruct the wrong key.
    ///
    /// # Arguments
    /// * `guardian_pks` - Ed25519 public keys of the designated guardians.
    /// * `threshold` - Minimum number of guardians needed to recover the key.
    ///
    /// # Errors
    /// Returns [`VauchiError::InvalidState`] if there are fewer guardians than
    /// the threshold, if a guardian key is invalid, or if shard parameters are
    /// out of range.
    pub fn export_guardian_backup_with_shards(
        &self,
        guardian_pks: &[[u8; 32]],
        threshold: u8,
    ) -> VauchiResult<(String, Vec<Vec<u8>>)> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let config = crate::backup::KeyShardConfig::new(threshold, guardian_pks.len() as u8)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;

        let identity_data = crate::backup::FullBackupIdentityData {
            display_name: identity.display_name().to_string(),
            master_seed: *identity.master_seed(),
            device_index: identity.device_index(),
            device_name: identity.device_info().device_name().to_string(),
        };

        let contacts = self.storage.contacts().list_contacts()?;
        let own_card = self.storage.contacts().load_own_card()?;
        let groups = self.storage.labels().load_all_groups()?;
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

        let backup_key = crate::backup::BackupKey::generate();
        let blob = crate::backup::export_guardian_backup(
            &identity_data,
            &contacts,
            own_card.as_ref(),
            &labels,
            backup_key.symmetric_key(),
            self.clock.unix_seconds(),
        )
        .map_err(|e| VauchiError::Configuration(format!("Guardian backup export failed: {e}")))?;

        let shards = crate::backup::split_backup_key(&backup_key, config)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;

        let mut sealed_shares = Vec::with_capacity(guardian_pks.len());
        for (shard, pk) in shards.iter().zip(guardian_pks.iter()) {
            let x25519_pk = ed25519_pk_to_x25519(pk)?;
            let sealed = crate::backup::seal_share_for_guardian(shard, &x25519_pk)
                .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
            sealed_shares.push(sealed);
        }

        // best-effort timestamp update
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.record_backup_completed();

        Ok((hex::encode(blob), sealed_shares))
    }

    /// Re-seals a guardian entry for a recovering party without exposing key
    /// material.
    ///
    /// A designated guardian calls this on receipt of a recovery request: it
    /// opens the share sealed to this identity's own key, then re-seals the
    /// plaintext share to `recovering_signing_pk`. The plaintext Shamir share
    /// and this identity's secret never leave Core — the returned bytes are
    /// ciphertext only, safe to hand back across the platform boundary
    /// (problem 2026-07-13-mobile-guardian-backup-integration; ADR-058).
    ///
    /// # Arguments
    /// * `sealed_share` - The share from
    ///   [`Self::export_guardian_backup_with_shards`] sealed to this guardian.
    /// * `recovering_signing_pk` - The recovering identity's Ed25519 signing
    ///   key (a recovery claim's `new_pk`).
    ///
    /// # Errors
    /// [`VauchiError::InvalidState`] if the share does not open with this
    /// identity; [`VauchiError::Crypto`] if `recovering_signing_pk` is invalid.
    pub fn respond_to_recovery(
        &self,
        sealed_share: &[u8],
        recovering_signing_pk: &[u8; 32],
    ) -> VauchiResult<Vec<u8>> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let our_secret = identity.signing_keypair().to_x25519_secret();
        let shard = crate::backup::open_share_for_guardian(sealed_share, &our_secret)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        let recipient = ed25519_pk_to_x25519(recovering_signing_pk)?;
        crate::backup::seal_share_for_guardian(&shard, &recipient)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))
    }

    /// Recovers a v4 guardian backup from re-sealed shares.
    ///
    /// Takes the encrypted backup blob (hex-encoded) and the shares that
    /// guardians re-sealed to this identity via [`Self::respond_to_recovery`].
    /// Opens each with this identity's own X25519 secret — derived in Core,
    /// never supplied by the caller — reconstructs the backup key, and returns
    /// the decrypted [`FullBackupEnvelope`]. The backup AEAD authenticates the
    /// reconstructed key before any plaintext is returned. No guardian secret
    /// or plaintext share ever crosses the platform boundary.
    ///
    /// **This method does not restore identity, contacts, or labels to storage.**
    /// The caller must apply the returned envelope the same way
    /// [`Self::import_full_backup`] applies a decrypted v3 backup, or call
    /// [`Self::import_full_backup`] if the backup was exported as a password
    /// backup instead.
    ///
    /// # Arguments
    /// * `backup_data` - Hex-encoded backup blob from [`Self::export_guardian_backup_with_shards`].
    /// * `re_sealed_shares` - Shares re-sealed to this identity by guardians.
    /// * `threshold` - Threshold selected when the guardian backup was exported.
    ///
    /// # Errors
    /// Returns [`VauchiError::InvalidState`] if share decryption or key
    /// reconstruction fails.
    pub fn recover_guardian_backup(
        &self,
        backup_data: &str,
        re_sealed_shares: &[Vec<u8>],
        threshold: u8,
    ) -> VauchiResult<crate::backup::FullBackupEnvelope> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let our_secret = identity.signing_keypair().to_x25519_secret();

        let mut shards = Vec::with_capacity(re_sealed_shares.len());
        for sealed in re_sealed_shares {
            let shard = crate::backup::open_share_for_guardian(sealed, &our_secret)
                .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
            shards.push(shard);
        }

        let backup_key = crate::backup::reconstruct_backup_key(&shards, threshold)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;

        let bytes = hex::decode(backup_data.trim())
            .map_err(|e| VauchiError::Configuration(format!("Invalid hex data: {e}")))?;

        crate::backup::import_guardian_backup(&bytes, backup_key.symmetric_key())
            .map_err(|e| VauchiError::Configuration(format!("Guardian backup import failed: {e}")))
    }
}

/// Converts an Ed25519 public key to an X25519 (Curve25519) public key.
///
/// Uses the birational map from Edwards to Montgomery form.
fn ed25519_pk_to_x25519(ed25519_pk: &[u8; 32]) -> VauchiResult<x25519_dalek::PublicKey> {
    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(ed25519_pk)
        .map_err(|e| VauchiError::Crypto(format!("invalid Ed25519 public key: {e}")))?;
    let montgomery = verifying_key.to_montgomery();
    Ok(x25519_dalek::PublicKey::from(montgomery.to_bytes()))
}
