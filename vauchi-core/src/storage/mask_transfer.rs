// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The mask profile carried in a full backup (ADR-080 point 7). The mask
//! key comes from the device's storage key, so a backup cannot carry the
//! seal: the mask's rows travel re-encrypted under a fresh transfer key,
//! inside the backup's real payload, and a restore re-encrypts them under
//! the new device's mask key.

use crate::crypto::SymmetricKey;

use super::duress_content::{decode_tables, insert_tables};
use super::mask_profile::NEVER_SEALED;
use super::{Storage, StorageError};

// INLINE_TEST_REQUIRED: the backup's private transfer must omit keys before
// encryption, and legacy transfer payloads cannot be built through the API.
#[cfg(all(test, feature = "notice-core"))]
#[path = "mask_transfer_tests.rs"]
mod tests;

/// The mask profile's rows under a transfer key that only the backup holds.
pub(crate) struct MaskTransfer {
    pub(crate) key: SymmetricKey,
    pub(crate) body: Vec<u8>,
}

impl Storage {
    /// This install's mask profile, re-encrypted under a fresh transfer key.
    pub(crate) fn mask_for_backup(&self) -> Result<MaskTransfer, StorageError> {
        let mask = self.mask_profile();
        let copy = Self::empty_mask_profile(mask.encryption_key.clone(), None)?;
        copy.load_body(&mask.sealed_body()?, &mask.encryption_key)?;
        copy.clear_transferred_device_state()?;
        let key = SymmetricKey::generate();
        copy.rekey_mask_rows(&mask.encryption_key, &key)?;
        Ok(MaskTransfer {
            key,
            body: copy.sealed_body()?,
        })
    }

    /// A fresh mask profile with a seed of its own and nothing else, for a
    /// decoy payload, whose restored install has no duress of its own.
    pub(crate) fn synthetic_mask_for_backup() -> Result<MaskTransfer, StorageError> {
        let key = SymmetricKey::generate();
        let mask = Self::empty_mask_profile(key.clone(), None)?;
        mask.mask_seed().ensure()?;
        Ok(MaskTransfer {
            key,
            body: mask.sealed_body()?,
        })
    }

    /// Replaces this install's mask profile with the one a backup carried,
    /// re-encrypted under this install's mask key, and seals it.
    pub(crate) fn restore_mask_from_backup(
        &self,
        transfer: &MaskTransfer,
    ) -> Result<(), StorageError> {
        let copy = Self::empty_mask_profile(transfer.key.clone(), None)?;
        copy.load_body(&transfer.body, &transfer.key)?;
        copy.clear_transferred_device_state()?;
        copy.renew_mask_device(self.clock.unix_seconds())?;
        let mask = self.mask_profile();
        copy.rekey_mask_rows(&transfer.key, &mask.encryption_key)?;
        let tables = decode_tables(&copy.sealed_body()?)?;
        mask.conn
            .execute_batch("BEGIN; PRAGMA defer_foreign_keys = ON;")?;
        let replaced = mask
            .clear_rows()
            .and_then(|()| insert_tables(&mask.conn, tables));
        match replaced {
            Ok(()) => mask.conn.execute_batch("COMMIT")?,
            Err(error) => {
                // The connection drops the transaction even when ROLLBACK
                // fails, so the error that caused it is the one to return.
                let _rolled_back = mask.conn.execute_batch("ROLLBACK").is_ok();
                return Err(error);
            }
        }
        self.seal_mask()
    }

    fn renew_mask_device(&self, now: u64) -> Result<(), StorageError> {
        use crate::identity::{DeviceRegistry, Identity};

        let old = self
            .mask_seed()
            .identity("", now)?
            .ok_or_else(|| StorageError::InvalidData("the mask profile has no seed".into()))?;
        let saved = self.device().load_device_registry_if_own(&old)?;
        let identity = loop {
            let index = u32::from_le_bytes(crate::crypto::random_bytes::<4>()) | 0x8000_0000;
            let candidate = Identity::from_device_link(
                *old.master_seed(),
                old.display_name().to_owned(),
                index,
                old.device_info().device_name().to_owned(),
                now,
            );
            if candidate.device_id() != old.device_id()
                && saved
                    .as_ref()
                    .is_none_or(|registry| registry.find_device(candidate.device_id()).is_none())
            {
                break candidate;
            }
        };
        // Saving only storage bytes preserves the real identity shown by the
        // mask's public_values (ADR-080), while retaining the new device index.
        self.identity()
            .save_identity(&identity.to_storage_bytes())?;
        let key = self.device().load_or_create_device_signing_key(&identity)?;
        let entry = identity
            .device_info()
            .to_registered(identity.master_seed())
            .with_signing_key(*key.public_key().as_bytes());
        let registry = match saved {
            Some(mut registry) => {
                registry
                    .register_restored_device(
                        entry,
                        old.device_id(),
                        identity.signing_keypair(),
                        now,
                    )
                    .map_err(|e| StorageError::InvalidData(e.to_string()))?;
                registry
            }
            None => {
                let mut registry = DeviceRegistry::new(entry, identity.signing_keypair());
                registry.lift_version_to(now, identity.signing_keypair());
                registry
            }
        };
        self.device().save_device_registry(&registry)
    }

    fn clear_transferred_device_state(&self) -> Result<(), StorageError> {
        // A restored mask is a new device, as a normal password restore is.
        // Old sessions and sealed outboxes belong to the previous device;
        // carrying them would fork its chains or reuse its activation (B5).
        self.conn.execute_batch(
            "DELETE FROM notice_states;
             DELETE FROM contact_ratchets;
             DELETE FROM registry_activation;
             DELETE FROM pending_updates;
             DELETE FROM retry_entries;
             DELETE FROM device_signing_key;
             DELETE FROM device_info;
             DELETE FROM device_sync_state;
             DELETE FROM device_sync_checkpoints;
             DELETE FROM sync_checkpoints;
             DELETE FROM contact_sync_timestamps;
             DELETE FROM receive_inbox;
             DELETE FROM receive_checkpoints;
             DELETE FROM receive_tokens_ahead;
             DELETE FROM receive_key_check;
             DELETE FROM resync_honored;",
        )?;
        Ok(())
    }

    fn clear_rows(&self) -> Result<(), StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table'
             AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for name in names {
            if !NEVER_SEALED.contains(&name.as_str()) {
                self.conn.execute(&format!("DELETE FROM {name}"), [])?;
            }
        }
        Ok(())
    }
}
