// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Identity domain persistence view.
//!
//! Owns the `identity` table. Part of problem record
//! `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).

use crate::clock::Clock;
use crate::crypto::SymmetricKey;
use rusqlite::{Connection, params};
use std::sync::Arc;

use super::super::{Storage, StorageError};

/// Scoped persistence view for the identity domain.
pub struct IdentityStore<'a> {
    conn: &'a Connection,
    duress_conn: &'a Connection,
    key: &'a SymmetricKey,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for the identity domain.
    pub fn identity(&self) -> IdentityStore<'_> {
        IdentityStore {
            conn: &self.conn,
            duress_conn: self.duress_conn(),
            key: &self.encryption_key,
            clock: &self.clock,
        }
    }
}

impl IdentityStore<'_> {
    fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }
    /// Saves identity backup data (encrypted).
    pub fn save_identity(&self, backup_data: &[u8]) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(self.key, backup_data)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO identity (id, backup_data_encrypted, created_at) VALUES (1, ?1, ?2)",
            params![encrypted, now as i64],
        )?;

        Ok(())
    }
    /// Loads identity backup data (decrypted), which carries the display
    /// name.
    pub fn load_identity(&self) -> Result<Option<Vec<u8>>, StorageError> {
        let result = self.conn.query_row(
            "SELECT backup_data_encrypted FROM identity WHERE id = 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        );

        match result {
            Ok(encrypted) => {
                let backup_data = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                Ok(Some(backup_data))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
    /// Checks if identity exists.
    pub fn has_identity(&self) -> Result<bool, StorageError> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM identity WHERE id = 1", [], |row| {
                    row.get(0)
                })?;
        Ok(count > 0)
    }
    /// Deletes the persisted identity row.
    ///
    /// Used by `Vauchi::perform_emergency_wipe` to clear identity from
    /// storage. After this call, `has_identity()` returns `false` and
    /// `load_identity()` returns `None`. Idempotent — succeeds even if
    /// no row exists.
    pub fn delete_identity(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM identity WHERE id = 1", [])?;
        Ok(())
    }
    /// Saves the app password hash and salt to the identity table.
    ///
    /// The hash is encrypted with the storage key before persisting. The
    /// first save also writes a duress cover, so the row has a duress salt
    /// and seal whether or not a duress PIN is ever set (ADR-032,
    /// vauchi/private#947).
    pub fn save_app_password(&self, hash: &[u8; 32], salt: &[u8; 16]) -> Result<(), StorageError> {
        let encrypted_hash = crate::crypto::encrypt(self.key, hash)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        self.conn.execute(
            "UPDATE identity SET password_hash_encrypted = ?1, password_salt = ?2 WHERE id = 1",
            params![encrypted_hash, salt.as_slice()],
        )?;
        if self.load_duress_seal()?.is_none() {
            self.write_duress_cover()?;
        }

        Ok(())
    }
    /// Saves the duress password hash and salt, and enables duress mode.
    ///
    /// The hash is sealed with the storage key together with the flags; a
    /// duress session's hold on the own-card copy survives a new PIN.
    pub fn save_duress_password(
        &self,
        hash: &[u8; 32],
        salt: &[u8; 16],
    ) -> Result<(), StorageError> {
        let copy_in_use = self
            .load_duress_seal()?
            .map_or(0, |seal| seal.flags & DURESS_COPY_IN_USE);
        self.write_duress_seal(DURESS_PIN_SET | copy_in_use, hash, salt)
    }
    /// Whether a duress session has taken the own-card copy over since the
    /// PIN was set; until then the copy starts again from the real card.
    pub fn duress_copy_in_use(&self) -> Result<bool, StorageError> {
        Ok(self
            .load_duress_seal()?
            .is_some_and(|seal| seal.flags & DURESS_COPY_IN_USE != 0))
    }
    /// Records that a duress session has taken the own-card copy over.
    pub fn mark_duress_copy_in_use(&self) -> Result<(), StorageError> {
        let seal = self
            .load_duress_seal()?
            .ok_or_else(|| StorageError::InvalidData("no duress seal".into()))?;
        self.write_duress_seal(seal.flags | DURESS_COPY_IN_USE, &seal.hash, &seal.salt)
    }
    /// Loads the password configuration from the identity table.
    ///
    /// Returns `None` if no password has been set (all password columns are NULL).
    pub fn load_password_config(
        &self,
    ) -> Result<Option<crate::identity::password_config::AppPasswordConfig>, StorageError> {
        let result = self.conn.query_row(
            "SELECT password_hash_encrypted, password_salt FROM identity WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<Vec<u8>>>(0)?,
                    row.get::<_, Option<Vec<u8>>>(1)?,
                ))
            },
        );

        match result {
            Ok((Some(hash_enc), Some(salt_bytes))) => {
                let hash_bytes = crate::crypto::decrypt(self.key, &hash_enc)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;

                let password_hash: [u8; 32] = hash_bytes.try_into().map_err(|_| {
                    StorageError::InvalidData("password hash has invalid length".into())
                })?;

                let password_salt: [u8; 16] = salt_bytes.try_into().map_err(|_| {
                    StorageError::InvalidData("password salt has invalid length".into())
                })?;

                let duress = self
                    .load_duress_seal()?
                    .filter(|seal| seal.flags & DURESS_PIN_SET != 0);

                Ok(Some(
                    crate::identity::password_config::AppPasswordConfig::from_stored(
                        password_hash,
                        password_salt,
                        duress.as_ref().map(|seal| seal.hash),
                        duress.as_ref().map(|seal| seal.salt),
                        duress.is_some(),
                    ),
                ))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
    /// Disables duress mode: the seal becomes a cover again, as for an
    /// identity that never had a PIN.
    pub fn disable_duress(&self) -> Result<(), StorageError> {
        self.write_duress_cover()?;
        // And so do the groups, tags and places its sessions made
        // (vauchi/private#813). The own-card copy stays, as every identity
        // has one; the caller starts it again from the real card.
        for table in [
            "duress_visibility_labels",
            "duress_contact_visibility_overrides",
            "duress_tags",
            "duress_places",
        ] {
            self.duress_conn
                .execute(&format!("DELETE FROM {table}"), [])?;
        }
        crate::storage::duress_content::write_sealed(self.conn, self.duress_conn, self.key)?;

        Ok(())
    }
    fn load_duress_seal(&self) -> Result<Option<DuressSeal>, StorageError> {
        let (sealed, salt) = match self.conn.query_row(
            "SELECT duress_hash_encrypted, duress_salt FROM identity WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<Vec<u8>>>(0)?,
                    row.get::<_, Option<Vec<u8>>>(1)?,
                ))
            },
        ) {
            Ok((Some(sealed), Some(salt))) => (sealed, salt),
            Ok(_) | Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(StorageError::Database(e)),
        };
        let opened = crate::crypto::decrypt(self.key, &sealed)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let Some((&flags, hash)) = opened.split_first() else {
            return Err(StorageError::InvalidData("duress seal is empty".into()));
        };
        Ok(Some(DuressSeal {
            flags,
            hash: hash
                .try_into()
                .map_err(|_| StorageError::InvalidData("duress seal has invalid length".into()))?,
            salt: salt
                .try_into()
                .map_err(|_| StorageError::InvalidData("duress salt has invalid length".into()))?,
        }))
    }
    fn write_duress_seal(
        &self,
        flags: u8,
        hash: &[u8; 32],
        salt: &[u8; 16],
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE identity SET duress_hash_encrypted = ?1, duress_salt = ?2, duress_enabled = 0 WHERE id = 1",
            params![seal_duress(self.key, flags, hash)?, salt.as_slice()],
        )?;
        Ok(())
    }
    fn write_duress_cover(&self) -> Result<(), StorageError> {
        self.write_duress_seal(
            0,
            &crate::crypto::random_bytes(),
            &crate::crypto::random_bytes(),
        )
    }
}

/// Seal flag: a duress PIN is set.
pub(crate) const DURESS_PIN_SET: u8 = 1;
/// Seal flag: a duress session has taken the own-card copy over.
pub(crate) const DURESS_COPY_IN_USE: u8 = 2;

/// The duress hash with its flags, as sealed in `duress_hash_encrypted`.
struct DuressSeal {
    flags: u8,
    hash: [u8; 32],
    salt: [u8; 16],
}

/// `flags || hash` under the storage key. Every identity with an app password
/// carries one, a random cover when no PIN is set, so the plain columns and
/// the blob's length are the same either way (ADR-032, vauchi/private#947).
pub(crate) fn seal_duress(
    key: &SymmetricKey,
    flags: u8,
    hash: &[u8; 32],
) -> Result<Vec<u8>, StorageError> {
    let mut plain = [0u8; 33];
    plain[0] = flags;
    plain[1..].copy_from_slice(hash);
    crate::crypto::encrypt(key, &plain).map_err(|e| StorageError::Encryption(e.to_string()))
}
