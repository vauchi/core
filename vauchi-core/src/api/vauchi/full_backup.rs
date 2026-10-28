// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Password full-backup capture and atomic restoration.

use crate::api::{VauchiError, VauchiResult};
use crate::backup::snapshot::{self, BackupSnapshot};
use crate::card::ContactCard;
use crate::identity::Identity;

use super::Vauchi;

impl Vauchi {
    /// Exports an encrypted v6 password backup, including sharing choices.
    ///
    /// Returns hex-encoded bytes. Readers predating v6 reject this format.
    pub fn export_full_backup(&self, password: &str) -> VauchiResult<String> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        // The same gate as the identity-only export: this file holds every
        // contact's shared key, so it must not be weaker (private#891).
        crate::identity::password::validate_password(password)
            .map_err(|e| VauchiError::Configuration(format!("Export failed: {:?}", e)))?;
        let snapshot = BackupSnapshot::capture(&self.storage, identity, self.clock.unix_seconds())
            .map_err(backup_error)?;
        snapshot.validate().map_err(backup_error)?;
        let blob = snapshot::encode(&snapshot, password).map_err(backup_error)?;
        // The completion timestamp is a UX hint, not part of backup integrity.
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.record_backup_completed();
        Ok(hex::encode(blob))
    }

    /// Atomically restores a password backup onto a fresh installation.
    ///
    /// Accepts v6 and legacy v3; missing legacy privacy state cannot be recovered.
    pub fn import_full_backup(&mut self, backup_data: &str, password: &str) -> VauchiResult<()> {
        if self.identity.is_some() {
            return Err(VauchiError::AlreadyInitialized);
        }
        let bytes = crate::backup::decode_full_backup_hex(backup_data).map_err(backup_error)?;
        let snapshot = snapshot::decode(&bytes, password).map_err(backup_error)?;
        let contacts = snapshot.validate().map_err(backup_error)?;
        let groups = snapshot.restore_groups();
        let seed = crate::backup::extract_master_seed(&snapshot.identity).map_err(backup_error)?;
        let identity = Identity::from_device_link(
            *seed,
            snapshot.identity.display_name.clone(),
            snapshot.identity.device_index,
            snapshot.identity.device_name.clone(),
            self.clock.unix_seconds(),
        );
        let fallback_card = ContactCard::new(identity.display_name());
        let own_card = snapshot.own_card.as_ref().unwrap_or(&fallback_card);

        self.storage.begin_transaction()?;
        let restored = (|| -> VauchiResult<()> {
            // Startup loads the storage encoding without the user's backup password.
            self.storage
                .identity()
                .save_identity(&identity.to_storage_bytes())?;
            self.storage.contacts().save_own_card(own_card)?;
            for contact in &contacts {
                self.storage.contacts().save_contact(contact)?;
            }
            for group in &groups {
                self.storage.labels().save_group(group)?;
            }
            for item in &snapshot.overrides {
                self.storage.labels().save_contact_override(
                    &item.contact_id,
                    &item.field_id,
                    item.visible,
                )?;
            }
            self.storage.commit()?;
            Ok(())
        })();
        if let Err(error) = restored {
            self.storage.rollback();
            return Err(error);
        }
        self.identity = Some(identity);
        Ok(())
    }
}

fn backup_error(error: crate::backup::BackupError) -> VauchiError {
    VauchiError::Configuration(format!("Full backup operation failed: {error}"))
}
