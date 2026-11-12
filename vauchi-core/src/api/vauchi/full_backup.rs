// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Password full-backup capture and atomic restoration.

use crate::api::{AuthResult, VauchiError, VauchiResult};
use crate::backup::snapshot::{self, BackupSnapshot, DuressSection, Payload};
use crate::card::ContactCard;
use crate::identity::{DeviceInfo, DeviceRegistry, Identity};

use super::ProfileEngine;

impl ProfileEngine {
    /// Exports an encrypted v6 password backup, including sharing choices.
    ///
    /// Returns hex-encoded bytes. Readers predating v6 reject this format.
    pub fn export_full_backup(&self, password: &str) -> VauchiResult<String> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        self.refuse_reused_passwords(&[password])?;
        // The same gate as the identity-only export: this file holds every
        // contact's shared key, so it must not be weaker (private#891).
        validate_backup_password(password)?;
        let snapshot = self.real_snapshot(identity)?;
        let blob = snapshot::encode(&snapshot, password).map_err(backup_error)?;
        Ok(self.finish_export(&blob))
    }

    /// Exports a v7 backup whose second payload opens with
    /// `decoy_password` and holds the decoy profile only (ADR-080 point 7).
    /// Only the real profile with a duress PIN has a decoy to export; a
    /// duress session is refused with the same error as an install
    /// without a duress PIN.
    pub fn export_full_backup_with_decoy(
        &self,
        password: &str,
        decoy_password: &str,
    ) -> VauchiResult<String> {
        let config = match self.password_config()? {
            Some(config) if !self.is_mask() && config.duress_enabled() => config,
            _ => return Err(no_decoy_to_export()),
        };
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        self.refuse_reused_passwords(&[password, decoy_password])?;
        validate_backup_password(password)?;
        validate_backup_password(decoy_password)?;
        let real = self.real_snapshot(identity)?;
        let decoy = BackupSnapshot::capture_decoy(
            &self.storage,
            &config,
            identity.display_name(),
            self.clock.unix_seconds(),
        )?;
        decoy.validate().map_err(backup_error)?;
        let blob = snapshot::encode_with_decoy(
            &Payload {
                snapshot: &real,
                password,
            },
            Some(&Payload {
                snapshot: &decoy,
                password: decoy_password,
            }),
        )
        .map_err(backup_error)?;
        Ok(self.finish_export(&blob))
    }

    /// The snapshot of the profile in front, with its duress section. On
    /// the mask, the export is shaped as a decoy payload is: locked with
    /// the duress PIN, with a fresh mask and no duress, so a backup made
    /// under duress restores as a decoy backup does (ADR-080 point 7).
    fn real_snapshot(&self, identity: &Identity) -> VauchiResult<BackupSnapshot> {
        let now = self.clock.unix_seconds();
        let config = self.password_config()?;
        let snapshot = match config {
            Some(config) if self.is_mask() => {
                BackupSnapshot::capture_decoy(&self.storage, &config, identity.display_name(), now)?
            }
            Some(config) => {
                let mut snapshot =
                    BackupSnapshot::capture(&self.storage, identity, now).map_err(backup_error)?;
                snapshot.duress = Some(DuressSection::capture(&self.storage, &config)?);
                snapshot
            }
            None => BackupSnapshot::capture(&self.storage, identity, now).map_err(backup_error)?,
        };
        snapshot.validate().map_err(backup_error)?;
        Ok(snapshot)
    }

    /// Refuses backup passwords that unlock this install, or that equal
    /// each other, since one guess would then open two layers (ADR-080
    /// point 7, D3). It runs before the strength check, so a duress PIN is
    /// refused for reuse under duress just as the app password is on the
    /// real profile.
    fn refuse_reused_passwords(&self, passwords: &[&str]) -> VauchiResult<()> {
        let config = self.password_config()?;
        let unlocks = |password: &str| {
            config
                .as_ref()
                .is_some_and(|c| !matches!(c.verify(password), AuthResult::Invalid))
        };
        let repeated = passwords
            .iter()
            .enumerate()
            .any(|(i, p)| passwords.iter().skip(i + 1).any(|q| q == p));
        if repeated || passwords.iter().any(|p| unlocks(p)) {
            return Err(VauchiError::Configuration(
                "Export failed: a backup password must differ from the app password, \
                 the duress PIN and the other backup password"
                    .into(),
            ));
        }
        Ok(())
    }

    fn finish_export(&self, blob: &[u8]) -> String {
        // The completion timestamp is a UX hint, not part of backup integrity.
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.record_backup_completed();
        hex::encode(blob)
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
        let now = self.clock.unix_seconds();
        let backed_up_device = device_id_at(&seed, snapshot.identity.device_index);
        // The restored install is a new device of the identity: signing as
        // the backed-up one with a fresh key would make every contact that
        // pinned that device refuse its deltas for good (vauchi/private#1247).
        let identity = Identity::from_device_link(
            *seed,
            snapshot.identity.display_name.clone(),
            restored_device_index(&seed, snapshot.device_registry.as_ref()),
            snapshot.identity.device_name.clone(),
            now,
        );
        let fallback_card = ContactCard::new(identity.display_name());
        let own_card = snapshot.own_card.as_ref().unwrap_or(&fallback_card);

        self.storage.begin_transaction()?;
        let restored = (|| -> VauchiResult<()> {
            // Startup loads the storage encoding without the user's backup password.
            self.storage.identity().save(&identity)?;
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
            if let Some(duress) = &snapshot.duress {
                duress.restore(&self.storage)?;
            }
            self.register_restored_device(
                &identity,
                snapshot.device_registry.clone(),
                &backed_up_device,
                now,
            )?;
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

impl ProfileEngine {
    /// Saves the registry the restored install signs: the backed-up one, or
    /// none for a backup that predates carrying it, with this install added
    /// as a new device. The next sync pushes it to every contact.
    fn register_restored_device(
        &self,
        identity: &Identity,
        backed_up: Option<DeviceRegistry>,
        backed_up_device: &[u8; 32],
        now: u64,
    ) -> VauchiResult<()> {
        let device_key = self.storage.device().load_or_create_device_signing_key()?;
        let entry = identity
            .device_info()
            .to_registered(identity.master_seed())
            .with_signing_key(*device_key.public_key().as_bytes());
        let registry = match backed_up {
            Some(mut registry) => {
                registry
                    .register_restored_device(
                        entry,
                        backed_up_device,
                        identity.signing_keypair(),
                        now,
                    )
                    .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
                registry
            }
            None => {
                let mut registry = DeviceRegistry::new(entry, identity.signing_keypair());
                registry.lift_version_to(now, identity.signing_keypair());
                registry
            }
        };
        self.storage.device().save_device_registry(&registry)?;
        Ok(())
    }
}

/// A device index no device of this identity has used. Indexes are handed
/// out from 0 as devices link, so a random one in the upper half of the
/// range stays clear of them and of any device a stale backup does not list.
fn restored_device_index(seed: &[u8; 32], registry: Option<&DeviceRegistry>) -> u32 {
    loop {
        let index = u32::from_le_bytes(crate::crypto::random_bytes::<4>()) | 0x8000_0000;
        let device_id = device_id_at(seed, index);
        if registry.is_none_or(|registry| registry.find_device(&device_id).is_none()) {
            return index;
        }
    }
}

fn device_id_at(seed: &[u8; 32], index: u32) -> [u8; 32] {
    *DeviceInfo::derive(seed, index, String::new(), 0).device_id()
}

fn backup_error(error: crate::backup::BackupError) -> VauchiError {
    VauchiError::Configuration(format!("Full backup operation failed: {error}"))
}

fn no_decoy_to_export() -> VauchiError {
    VauchiError::InvalidState("no decoy profile to export".into())
}

fn validate_backup_password(password: &str) -> VauchiResult<()> {
    crate::identity::password::validate_password(password)
        .map(|_| ())
        .map_err(|e| VauchiError::Configuration(format!("Export failed: {:?}", e)))
}
