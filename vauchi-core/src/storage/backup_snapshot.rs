// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Storage capture adapter for the pure backup representation.

use super::{MaskTransfer, Storage, StorageError};
use crate::backup::{
    BackupError,
    snapshot::{BackupSnapshot, DuressPinFlags, DuressSection, VisibilityOverride},
};
use crate::emergency::DuressUnlock;
use crate::identity::Identity;
use crate::identity::password_config::AppPasswordConfig;

impl BackupSnapshot {
    pub fn capture(storage: &Storage, identity: &Identity, now: u64) -> Result<Self, BackupError> {
        let map_error = |_| BackupError::Serialization("Could not read backup state".into());
        let mut contacts = storage.contacts().list_contacts().map_err(map_error)?;
        contacts.extend(
            storage
                .contacts()
                .list_archived_contacts()
                .map_err(map_error)?,
        );
        let groups = storage.labels().load_all_groups().map_err(map_error)?;
        let contact_ids: std::collections::HashSet<_> = contacts.iter().map(|c| c.id()).collect();
        let mut overrides = Vec::new();
        for (contact_id, fields) in storage
            .labels()
            .load_all_contact_overrides()
            .map_err(map_error)?
        {
            if contact_ids.contains(contact_id.as_str()) {
                overrides.extend(fields.into_iter().map(|(field_id, visible)| {
                    VisibilityOverride {
                        contact_id: contact_id.clone(),
                        field_id,
                        visible,
                    }
                }));
            }
        }
        Self::from_captured_state(
            identity,
            now,
            storage.contacts().load_own_card().map_err(map_error)?,
            contacts,
            groups,
            overrides,
        )
    }
}

impl DuressSection {
    /// The duress section of `storage`'s install, whose password
    /// configuration is `config`.
    pub(crate) fn capture(
        storage: &Storage,
        config: &AppPasswordConfig,
    ) -> Result<Self, StorageError> {
        let transfer = storage.mask_for_backup()?;
        let duress_pin = match (
            config.duress_enabled(),
            config.duress_hash(),
            config.duress_salt(),
        ) {
            (true, Some(hash), Some(salt)) => Some((
                (hash, salt),
                DuressPinFlags {
                    copy_in_use: storage.identity().duress_copy_in_use()?,
                    password_branch: storage.identity().duress_unlock()?
                        == Some(DuressUnlock::Passwords),
                },
            )),
            _ => None,
        };
        Ok(Self::new(
            &transfer.key,
            &transfer.body,
            (config.password_hash(), config.password_salt()),
            duress_pin,
        ))
    }

    /// Writes the credentials and the mask into `storage`, a fresh install
    /// whose identity row the restore has just saved.
    pub(crate) fn restore(&self, storage: &Storage) -> Result<(), StorageError> {
        let invalid = |e: BackupError| StorageError::InvalidData(e.to_string());
        let password = self.password().map_err(invalid)?;
        storage
            .identity()
            .save_app_password(&password.hash, &password.salt)?;
        if let Some(pin) = self.duress_pin().map_err(invalid)? {
            storage.identity().save_duress_password_under(
                &pin.credential.hash,
                &pin.credential.salt,
                if pin.password_branch {
                    DuressUnlock::Passwords
                } else {
                    DuressUnlock::Biometric
                },
            )?;
            if pin.copy_in_use {
                storage.identity().mark_duress_copy_in_use()?;
            }
        }
        storage.restore_mask_from_backup(&MaskTransfer {
            key: self.mask_key().map_err(invalid)?,
            body: self.mask_body().map_err(invalid)?,
        })
    }
}

impl BackupSnapshot {
    /// The decoy payload's snapshot: the mask profile under the mask
    /// identity, locked with the duress PIN as its app password, with a
    /// fresh mask of its own and no duress (ADR-080 point 7, amended
    /// 2026-11-09).
    pub(crate) fn capture_decoy(
        storage: &Storage,
        config: &AppPasswordConfig,
        display_name: &str,
        now: u64,
    ) -> Result<Self, StorageError> {
        let (Some(hash), Some(salt)) = (config.duress_hash(), config.duress_salt()) else {
            return Err(StorageError::InvalidData(
                "no duress PIN to lock a decoy".into(),
            ));
        };
        let seed = storage
            .mask_seed()
            .load()?
            .ok_or_else(|| StorageError::InvalidData("the mask profile has no seed".into()))?;
        let identity = Identity::for_mask(seed.as_bytes(), display_name, now);
        let mut snapshot = Self::capture(storage.mask_profile(), &identity, now)
            .map_err(|e| StorageError::InvalidData(e.to_string()))?;
        let synthetic = Storage::synthetic_mask_for_backup()?;
        snapshot.duress = Some(DuressSection::new(
            &synthetic.key,
            &synthetic.body,
            (hash, salt),
            None,
        ));
        Ok(snapshot)
    }
}
