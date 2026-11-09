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
