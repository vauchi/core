// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Persistent Storage Module
//!
//! Provides encrypted local storage for contacts, identity, and sync state.
//! Uses SQLite with application-level encryption for sensitive data.

#[cfg(feature = "testing")]
pub mod consent;
#[cfg(not(feature = "testing"))]
mod consent;

#[cfg(feature = "testing")]
pub mod contacts;
#[cfg(not(feature = "testing"))]
mod contacts;

#[cfg(feature = "testing")]
pub mod device;
#[cfg(not(feature = "testing"))]
mod device;

#[cfg(feature = "testing")]
pub mod error;
#[cfg(not(feature = "testing"))]
mod error;

#[cfg(feature = "testing")]
pub mod identity;
#[cfg(not(feature = "testing"))]
mod identity;

#[cfg(feature = "testing")]
pub mod labels;
#[cfg(not(feature = "testing"))]
mod labels;

#[cfg(feature = "testing")]
pub mod pending;
#[cfg(not(feature = "testing"))]
mod pending;

#[cfg(feature = "testing")]
pub mod delivery;
#[cfg(not(feature = "testing"))]
mod delivery;

#[cfg(feature = "testing")]
pub mod retry;
#[cfg(not(feature = "testing"))]
mod retry;

#[cfg(feature = "testing")]
pub mod device_delivery;
#[cfg(not(feature = "testing"))]
mod device_delivery;

#[cfg(feature = "testing")]
pub mod ratchet;
#[cfg(not(feature = "testing"))]
mod ratchet;

#[cfg(feature = "testing")]
pub mod validation;
#[cfg(not(feature = "testing"))]
mod validation;

#[cfg(feature = "testing")]
pub mod recovery;
#[cfg(not(feature = "testing"))]
mod recovery;

#[cfg(feature = "testing")]
pub mod ux;
#[cfg(not(feature = "testing"))]
mod ux;

#[cfg(feature = "testing")]
pub mod decoy;
#[cfg(not(feature = "testing"))]
mod decoy;

#[cfg(feature = "testing")]
pub mod duress_settings;
#[cfg(not(feature = "testing"))]
mod duress_settings;

#[cfg(feature = "testing")]
pub mod emergency;
#[cfg(not(feature = "testing"))]
mod emergency;

pub mod migration;
pub mod secure;

pub use error::{
    DeletionState, DeliveryRecord, DeliveryStatus, DeliverySummary, DeviceDeliveryRecord,
    DeviceDeliveryStatus, OfflineQueue, PendingUpdate, RetryEntry, RetryQueue, StorageError,
    UpdateStatus,
};
pub use secure::{FileKeyStorage, SecureStorage};

#[cfg(any(test, feature = "testing"))]
pub use secure::MemoryKeyStorage;

#[cfg(feature = "secure-storage")]
pub use secure::PlatformKeyring;

use ring::hmac;
use rusqlite::Connection;
use std::path::Path;

use crate::crypto::{SymmetricKey, HKDF};

/// SQLite-based storage implementation.
///
/// Stores data in a local SQLite database with application-level encryption
/// for sensitive fields (keys, cards, etc.).
///
/// # Thread Safety (#80)
///
/// `Storage` is intentionally **not `Send`** because `rusqlite::Connection`
/// is not `Send`. Each client creates its own `Storage` instance on its
/// thread. For async contexts, wrap in `tokio::task::spawn_blocking` or use
/// a dedicated storage thread with a channel. The UniFFI mobile bindings
/// open a fresh storage per call via `open_vauchi()`.
pub struct Storage {
    conn: Connection,
    /// Encryption key derived from user's master key
    pub(crate) encryption_key: SymmetricKey,
    /// Database file path (None for in-memory databases).
    db_path: Option<std::path::PathBuf>,
}

impl Storage {
    /// Opens or creates a storage database at the given path.
    pub fn open<P: AsRef<Path>>(
        path: P,
        encryption_key: SymmetricKey,
    ) -> Result<Self, StorageError> {
        let path_buf = path.as_ref().to_path_buf();
        let conn = Connection::open(&path_buf)?;
        Self::configure_pragmas(&conn)?;
        let storage = Storage {
            conn,
            encryption_key,
            db_path: Some(path_buf),
        };
        storage.run_migrations()?;
        Ok(storage)
    }

    /// Creates an in-memory storage (for testing).
    pub fn in_memory(encryption_key: SymmetricKey) -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        Self::configure_pragmas(&conn)?;
        let storage = Storage {
            conn,
            encryption_key,
            db_path: None,
        };
        storage.run_migrations()?;
        Ok(storage)
    }

    /// Configures SQLite PRAGMAs for performance and security.
    ///
    /// Performance:
    /// - WAL mode: enables concurrent reads during writes
    /// - busy_timeout=5000: wait up to 5s for locks instead of failing immediately
    /// - synchronous=NORMAL: safe with WAL, better write throughput
    /// - cache_size=10000: larger page cache for query performance
    ///
    /// Security (defense-in-depth for crypto-shredding):
    /// - secure_delete=ON: overwrites deleted content with zeros
    /// - auto_vacuum=FULL: reclaims and overwrites freed pages on delete
    /// - temp_store=MEMORY: keeps temporary tables in RAM, not on disk
    ///
    /// Note: secure_delete is partially negated by WAL mode (pre-modification
    /// data persists in WAL file). The primary protection is the SMK encryption
    /// layer; these PRAGMAs are secondary defense-in-depth.
    fn configure_pragmas(conn: &Connection) -> Result<(), StorageError> {
        // auto_vacuum must be set before any tables are created (before first
        // page write), so it comes first — before journal_mode=WAL which writes
        // the database header.
        conn.execute_batch(
            "PRAGMA auto_vacuum=FULL;
             PRAGMA busy_timeout=5000;
             PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA cache_size=10000;
             PRAGMA secure_delete=ON;
             PRAGMA temp_store=MEMORY;",
        )?;
        Ok(())
    }

    /// Runs all pending schema migrations.
    ///
    /// For file-based databases, creates a pre-migration backup before
    /// applying pending migrations (#17).
    fn run_migrations(&self) -> Result<(), StorageError> {
        let migrations = migration::all_migrations();
        migration::MigrationRunner::run(
            &self.conn,
            &self.encryption_key,
            &migrations,
            self.db_path.as_deref(),
        )
    }

    /// Returns the current schema version.
    pub fn schema_version(&self) -> Result<u32, StorageError> {
        migration::MigrationRunner::current_version(&self.conn)
    }

    /// Computes a deterministic HMAC for encrypted column lookups (#128).
    ///
    /// Derives a dedicated HMAC key from the SEK via HKDF, then computes
    /// HMAC-SHA256(hmac_key, data). This allows equality lookups on encrypted
    /// data without decryption (e.g., label name uniqueness checks).
    pub(crate) fn compute_lookup_hmac(&self, domain: &[u8], data: &[u8]) -> Vec<u8> {
        let hmac_key_bytes = HKDF::derive_key(None, self.encryption_key.as_bytes(), domain);
        let key = hmac::Key::new(hmac::HMAC_SHA256, &hmac_key_bytes);
        hmac::sign(&key, data).as_ref().to_vec()
    }

    /// Forces a WAL checkpoint, merging all WAL frames into the main DB file (#81).
    ///
    /// TRUNCATE mode flushes WAL contents and truncates the WAL file to zero bytes.
    /// This must be called before secure delete or rekey operations to ensure that
    /// pre-modification data in the WAL is merged and overwritable.
    pub fn wal_checkpoint(&self) -> Result<(), StorageError> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| StorageError::Migration(format!("WAL checkpoint: {}", e)))
    }

    /// Re-encrypts all encrypted columns from the current key to a new key.
    ///
    /// This is used during SMK migration: the database was opened with the old
    /// storage_key, and all data needs to be re-encrypted with the new SEK
    /// derived from SMK. After successful rekey, the internal encryption_key
    /// is updated to the new key.
    ///
    /// The operation runs in a single transaction for atomicity — if any step
    /// fails, all changes are rolled back and the old key remains valid.
    ///
    /// The optional `progress` callback receives `(completed_tables, total_tables, table_name)`
    /// after each table is re-encrypted (#166a).
    pub fn rekey(&mut self, new_key: SymmetricKey) -> Result<(), StorageError> {
        self.rekey_with_progress(new_key, None)
    }

    /// Re-encrypts all encrypted columns with progress reporting (#166a).
    ///
    /// See [`rekey`] for details. The `progress` callback, if provided,
    /// is called after each table completes with `(completed, total, table_name)`.
    #[allow(clippy::type_complexity)]
    pub fn rekey_with_progress(
        &mut self,
        new_key: SymmetricKey,
        progress: Option<&dyn Fn(u32, u32, &str)>,
    ) -> Result<(), StorageError> {
        use crate::crypto::{decrypt, encrypt};
        use rusqlite::params;

        // Flush WAL before rekey to ensure all data is in the main DB file (#129)
        self.wal_checkpoint()?;

        let old_key = &self.encryption_key;
        const TOTAL_TABLES: u32 = 20;
        let mut completed: u32 = 0;

        let report = |completed: &mut u32, table: &str| {
            *completed += 1;
            if let Some(cb) = &progress {
                cb(*completed, TOTAL_TABLES, table);
            }
        };

        self.conn.execute_batch("BEGIN EXCLUSIVE TRANSACTION")?;

        let result = (|| -> Result<(), StorageError> {
            // Re-encrypt contacts: card_encrypted and shared_key_encrypted
            {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id, card_encrypted, shared_key_encrypted FROM contacts")
                    .map_err(|e| StorageError::Migration(format!("Read contacts: {}", e)))?;

                let rows: Vec<(String, Vec<u8>, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                    .map_err(|e| StorageError::Migration(format!("Query contacts: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect contacts: {}", e)))?;

                for (id, card_enc, key_enc) in &rows {
                    let card_plain = decrypt(old_key, card_enc).map_err(|e| {
                        StorageError::Migration(format!("Decrypt card {}: {}", id, e))
                    })?;
                    let key_plain = decrypt(old_key, key_enc).map_err(|e| {
                        StorageError::Migration(format!("Decrypt key {}: {}", id, e))
                    })?;

                    let card_new = encrypt(&new_key, &card_plain).map_err(|e| {
                        StorageError::Migration(format!("Encrypt card {}: {}", id, e))
                    })?;
                    let key_new = encrypt(&new_key, &key_plain).map_err(|e| {
                        StorageError::Migration(format!("Encrypt key {}: {}", id, e))
                    })?;

                    self.conn.execute(
                        "UPDATE contacts SET card_encrypted = ?1, shared_key_encrypted = ?2 WHERE id = ?3",
                        params![card_new, key_new, id],
                    ).map_err(|e| StorageError::Migration(format!("Update contact {}: {}", id, e)))?;
                }
            }
            report(&mut completed, "contacts");

            // Re-encrypt contacts: personal_notes_encrypted and avatar_encrypted (nullable)
            {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id, personal_notes_encrypted, avatar_encrypted FROM contacts WHERE personal_notes_encrypted IS NOT NULL OR avatar_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read contact extras: {}", e)))?;

                type ContactExtras = (String, Option<Vec<u8>>, Option<Vec<u8>>);
                let rows: Vec<ContactExtras> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                    .map_err(|e| StorageError::Migration(format!("Query contact extras: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| {
                        StorageError::Migration(format!("Collect contact extras: {}", e))
                    })?;

                for (id, notes_enc, avatar_enc) in &rows {
                    let notes_new = if let Some(enc) = notes_enc {
                        let plain = decrypt(old_key, enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt notes {}: {}", id, e))
                        })?;
                        Some(encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt notes {}: {}", id, e))
                        })?)
                    } else {
                        None
                    };

                    let avatar_new = if let Some(enc) = avatar_enc {
                        let plain = decrypt(old_key, enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt avatar {}: {}", id, e))
                        })?;
                        Some(encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt avatar {}: {}", id, e))
                        })?)
                    } else {
                        None
                    };

                    self.conn.execute(
                        "UPDATE contacts SET personal_notes_encrypted = ?1, avatar_encrypted = ?2 WHERE id = ?3",
                        params![notes_new, avatar_new, id],
                    ).map_err(|e| StorageError::Migration(format!("Update contact extras {}: {}", id, e)))?;
                }
            }
            report(&mut completed, "contact_extras");

            // Re-encrypt identity: backup_data_encrypted
            {
                let result: Result<(i64, Vec<u8>), _> = self.conn.query_row(
                    "SELECT id, backup_data_encrypted FROM identity WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                );

                if let Ok((id, backup_enc)) = result {
                    let plain = decrypt(old_key, &backup_enc)
                        .map_err(|e| StorageError::Migration(format!("Decrypt identity: {}", e)))?;
                    let new_enc = encrypt(&new_key, &plain)
                        .map_err(|e| StorageError::Migration(format!("Encrypt identity: {}", e)))?;
                    self.conn
                        .execute(
                            "UPDATE identity SET backup_data_encrypted = ?1 WHERE id = ?2",
                            params![new_enc, id],
                        )
                        .map_err(|e| StorageError::Migration(format!("Update identity: {}", e)))?;
                }
            }
            report(&mut completed, "identity");

            // Re-encrypt ratchet state with per-contact derived keys (#126)
            {
                use crate::crypto::kdf::HKDF;

                let mut stmt = self
                    .conn
                    .prepare("SELECT contact_id, ratchet_state_encrypted FROM contact_ratchets")
                    .map_err(|e| StorageError::Migration(format!("Read ratchets: {}", e)))?;

                let rows: Vec<(String, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query ratchets: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect ratchets: {}", e)))?;

                for (contact_id, ratchet_enc) in &rows {
                    // Decrypt with old per-contact key
                    let mut old_info = b"vauchi-ratchet-storage-v1:".to_vec();
                    old_info.extend_from_slice(contact_id.as_bytes());
                    let old_derived = HKDF::derive_key(None, old_key.as_bytes(), &old_info);
                    let old_ratchet_key = SymmetricKey::from_bytes(old_derived);

                    let plain = decrypt(&old_ratchet_key, ratchet_enc).map_err(|e| {
                        StorageError::Migration(format!("Decrypt ratchet {}: {}", contact_id, e))
                    })?;

                    // Re-encrypt with new per-contact key
                    let mut new_info = b"vauchi-ratchet-storage-v1:".to_vec();
                    new_info.extend_from_slice(contact_id.as_bytes());
                    let new_derived = HKDF::derive_key(None, new_key.as_bytes(), &new_info);
                    let new_ratchet_key = SymmetricKey::from_bytes(new_derived);

                    let new_enc = encrypt(&new_ratchet_key, &plain).map_err(|e| {
                        StorageError::Migration(format!("Encrypt ratchet {}: {}", contact_id, e))
                    })?;
                    self.conn.execute(
                        "UPDATE contact_ratchets SET ratchet_state_encrypted = ?1 WHERE contact_id = ?2",
                        params![new_enc, contact_id],
                    ).map_err(|e| StorageError::Migration(format!("Update ratchet {}: {}", contact_id, e)))?;
                }
            }
            report(&mut completed, "ratchets");

            // Re-encrypt own_card: card_json_encrypted
            {
                let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
                    "SELECT card_json_encrypted FROM own_card WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?,)),
                );

                if let Ok((Some(enc),)) = result {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, &enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt own_card: {}", e))
                        })?;
                        let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt own_card: {}", e))
                        })?;
                        self.conn
                            .execute(
                                "UPDATE own_card SET card_json_encrypted = ?1 WHERE id = 1",
                                params![new_enc],
                            )
                            .map_err(|e| {
                                StorageError::Migration(format!("Update own_card: {}", e))
                            })?;
                    }
                }
            }
            report(&mut completed, "own_card");

            // Re-encrypt device_registry: registry_json_encrypted
            {
                let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
                    "SELECT registry_json_encrypted FROM device_registry WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?,)),
                );

                if let Ok((Some(enc),)) = result {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, &enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt registry: {}", e))
                        })?;
                        let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt registry: {}", e))
                        })?;
                        self.conn.execute(
                            "UPDATE device_registry SET registry_json_encrypted = ?1 WHERE id = 1",
                            params![new_enc],
                        ).map_err(|e| StorageError::Migration(format!("Update registry: {}", e)))?;
                    }
                }
            }
            report(&mut completed, "device_registry");

            // Re-encrypt device_sync_state: state_json_encrypted
            {
                let mut stmt = self
                    .conn
                    .prepare("SELECT device_id, state_json_encrypted FROM device_sync_state WHERE state_json_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read device_sync: {}", e)))?;

                let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query device_sync: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect device_sync: {}", e)))?;

                for (device_id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt device_sync: {}", e))
                        })?;
                        let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt device_sync: {}", e))
                        })?;
                        self.conn.execute(
                            "UPDATE device_sync_state SET state_json_encrypted = ?1 WHERE device_id = ?2",
                            params![new_enc, device_id],
                        ).map_err(|e| StorageError::Migration(format!("Update device_sync: {}", e)))?;
                    }
                }
            }
            report(&mut completed, "device_sync_state");

            // Re-encrypt visibility_labels: contacts_json_encrypted, visible_fields_json_encrypted, name_encrypted, name_hmac
            {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id, contacts_json_encrypted, visible_fields_json_encrypted, name_encrypted FROM visibility_labels WHERE contacts_json_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read labels: {}", e)))?;

                #[allow(clippy::type_complexity)]
                let rows: Vec<(String, Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>)> = stmt
                    .query_map([], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                    })
                    .map_err(|e| StorageError::Migration(format!("Query labels: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect labels: {}", e)))?;

                // Derive HMAC keys for old and new SEK
                let new_hmac_key_bytes =
                    HKDF::derive_key(None, new_key.as_bytes(), b"Vauchi_Label_Name_HMAC_v1");
                let new_hmac_key = hmac::Key::new(hmac::HMAC_SHA256, &new_hmac_key_bytes);

                for (id, contacts_enc, fields_enc, name_enc) in &rows {
                    let contacts_new = if !contacts_enc.is_empty() {
                        let plain = decrypt(old_key, contacts_enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt label contacts {}: {}", id, e))
                        })?;
                        encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt label contacts {}: {}", id, e))
                        })?
                    } else {
                        contacts_enc.clone()
                    };

                    let fields_new = if let Some(enc) = fields_enc {
                        if !enc.is_empty() {
                            let plain = decrypt(old_key, enc).map_err(|e| {
                                StorageError::Migration(format!(
                                    "Decrypt label fields {}: {}",
                                    id, e
                                ))
                            })?;
                            Some(encrypt(&new_key, &plain).map_err(|e| {
                                StorageError::Migration(format!(
                                    "Encrypt label fields {}: {}",
                                    id, e
                                ))
                            })?)
                        } else {
                            Some(enc.clone())
                        }
                    } else {
                        None
                    };

                    // Re-encrypt name and recompute HMAC (#128)
                    let (name_new, name_hmac_new) = if let Some(enc) = name_enc {
                        if !enc.is_empty() {
                            let plain = decrypt(old_key, enc).map_err(|e| {
                                StorageError::Migration(format!("Decrypt label name {}: {}", id, e))
                            })?;
                            let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                                StorageError::Migration(format!("Encrypt label name {}: {}", id, e))
                            })?;
                            let hmac_val = hmac::sign(&new_hmac_key, &plain);
                            (Some(new_enc), Some(hmac_val.as_ref().to_vec()))
                        } else {
                            (Some(enc.clone()), None)
                        }
                    } else {
                        (None, None)
                    };

                    self.conn.execute(
                        "UPDATE visibility_labels SET contacts_json_encrypted = ?1, visible_fields_json_encrypted = ?2, name_encrypted = ?3, name_hmac = ?4 WHERE id = ?5",
                        params![contacts_new, fields_new, name_new, name_hmac_new, id],
                    ).map_err(|e| StorageError::Migration(format!("Update label {}: {}", id, e)))?;
                }
            }
            report(&mut completed, "visibility_labels");

            // Re-encrypt device_info: device_info_encrypted
            {
                let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
                    "SELECT device_info_encrypted FROM device_info WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?,)),
                );

                if let Ok((Some(enc),)) = result {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, &enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt device_info: {}", e))
                        })?;
                        let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt device_info: {}", e))
                        })?;
                        self.conn
                            .execute(
                                "UPDATE device_info SET device_info_encrypted = ?1 WHERE id = 1",
                                params![new_enc],
                            )
                            .map_err(|e| {
                                StorageError::Migration(format!("Update device_info: {}", e))
                            })?;
                    }
                }
            }
            report(&mut completed, "device_info");

            // Re-encrypt version_vector: vector_json_encrypted
            {
                let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
                    "SELECT vector_json_encrypted FROM version_vector WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?,)),
                );

                if let Ok((Some(enc),)) = result {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, &enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt version_vector: {}", e))
                        })?;
                        let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt version_vector: {}", e))
                        })?;
                        self.conn
                            .execute(
                                "UPDATE version_vector SET vector_json_encrypted = ?1 WHERE id = 1",
                                params![new_enc],
                            )
                            .map_err(|e| {
                                StorageError::Migration(format!("Update version_vector: {}", e))
                            })?;
                    }
                }
            }
            report(&mut completed, "version_vector");

            // Re-encrypt contact_sync_timestamps: last_sync_at_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT contact_id, last_sync_at_encrypted FROM contact_sync_timestamps WHERE last_sync_at_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read sync_timestamps: {}", e)))?;

                let rows: Vec<(String, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query sync_timestamps: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| {
                        StorageError::Migration(format!("Collect sync_timestamps: {}", e))
                    })?;

                for (contact_id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt sync_ts: {}", e))
                        })?;
                        let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt sync_ts: {}", e))
                        })?;
                        self.conn.execute(
                            "UPDATE contact_sync_timestamps SET last_sync_at_encrypted = ?1 WHERE contact_id = ?2",
                            params![new_enc, contact_id],
                        ).map_err(|e| StorageError::Migration(format!("Update sync_ts: {}", e)))?;
                    }
                }
            }
            report(&mut completed, "sync_timestamps");

            // Re-encrypt pending_updates: payload_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT id, payload_encrypted FROM pending_updates WHERE payload_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read pending: {}", e)))?;

                let rows: Vec<(String, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query pending: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect pending: {}", e)))?;

                for (id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt pending {}: {}", id, e))
                        })?;
                        let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt pending {}: {}", id, e))
                        })?;
                        self.conn
                            .execute(
                                "UPDATE pending_updates SET payload_encrypted = ?1 WHERE id = ?2",
                                params![new_enc, id],
                            )
                            .map_err(|e| {
                                StorageError::Migration(format!("Update pending {}: {}", id, e))
                            })?;
                    }
                }
            }
            report(&mut completed, "pending_updates");

            // Re-encrypt retry_entries: payload_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT message_id, payload_encrypted FROM retry_entries WHERE payload_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read retry: {}", e)))?;

                let rows: Vec<(String, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query retry: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect retry: {}", e)))?;

                for (id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt retry {}: {}", id, e))
                        })?;
                        let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt retry {}: {}", id, e))
                        })?;
                        self.conn.execute(
                            "UPDATE retry_entries SET payload_encrypted = ?1 WHERE message_id = ?2",
                            params![new_enc, id],
                        ).map_err(|e| StorageError::Migration(format!("Update retry {}: {}", id, e)))?;
                    }
                }
            }
            report(&mut completed, "retry_entries");

            // Re-encrypt device_sync_checkpoints: items_json_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT target_device_id, items_json_encrypted FROM device_sync_checkpoints WHERE items_json_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read checkpoints: {}", e)))?;

                let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query checkpoints: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect checkpoints: {}", e)))?;

                for (device_id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt checkpoint: {}", e))
                        })?;
                        let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt checkpoint: {}", e))
                        })?;
                        self.conn.execute(
                            "UPDATE device_sync_checkpoints SET items_json_encrypted = ?1 WHERE target_device_id = ?2",
                            params![new_enc, device_id],
                        ).map_err(|e| StorageError::Migration(format!("Update checkpoint: {}", e)))?;
                    }
                }
            }
            report(&mut completed, "device_sync_checkpoints");

            // Re-encrypt recovery_responses: response_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT claim_id, response_encrypted FROM recovery_responses WHERE response_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read recovery: {}", e)))?;

                let rows: Vec<(String, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query recovery: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect recovery: {}", e)))?;

                for (id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt recovery {}: {}", id, e))
                        })?;
                        let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt recovery {}: {}", id, e))
                        })?;
                        self.conn.execute(
                            "UPDATE recovery_responses SET response_encrypted = ?1 WHERE claim_id = ?2",
                            params![new_enc, id],
                        ).map_err(|e| StorageError::Migration(format!("Update recovery {}: {}", id, e)))?;
                    }
                }
            }
            report(&mut completed, "recovery_responses");

            // Re-encrypt deletion_state: state_json_encrypted
            {
                let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
                    "SELECT state_json_encrypted FROM deletion_state WHERE id = 1",
                    [],
                    |row| Ok((row.get(0)?,)),
                );

                if let Ok((Some(enc),)) = result {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, &enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt deletion_state: {}", e))
                        })?;
                        let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt deletion_state: {}", e))
                        })?;
                        self.conn
                            .execute(
                                "UPDATE deletion_state SET state_json_encrypted = ?1 WHERE id = 1",
                                params![new_enc],
                            )
                            .map_err(|e| {
                                StorageError::Migration(format!("Update deletion_state: {}", e))
                            })?;
                    }
                }
            }
            report(&mut completed, "deletion_state");

            // Re-encrypt sync_checkpoints: state_json_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT checkpoint_id, state_json_encrypted FROM sync_checkpoints WHERE state_json_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read batch_checkpoints: {}", e)))?;

                let rows: Vec<(String, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| {
                        StorageError::Migration(format!("Query batch_checkpoints: {}", e))
                    })?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| {
                        StorageError::Migration(format!("Collect batch_checkpoints: {}", e))
                    })?;

                for (id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc).map_err(|e| {
                            StorageError::Migration(format!(
                                "Decrypt batch_checkpoint {}: {}",
                                id, e
                            ))
                        })?;
                        let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!(
                                "Encrypt batch_checkpoint {}: {}",
                                id, e
                            ))
                        })?;
                        self.conn.execute(
                            "UPDATE sync_checkpoints SET state_json_encrypted = ?1 WHERE checkpoint_id = ?2",
                            params![new_enc, id],
                        ).map_err(|e| StorageError::Migration(format!("Update batch_checkpoint {}: {}", id, e)))?;
                    }
                }
            }
            report(&mut completed, "sync_checkpoints");

            // Re-encrypt field_validations: field_value_encrypted, signature_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT id, field_value_encrypted, signature_encrypted FROM field_validations")
                    .map_err(|e| StorageError::Migration(format!("Read field_validations for rekey: {}", e)))?;
                #[allow(clippy::type_complexity)]
                let rows: Vec<(String, Option<Vec<u8>>, Option<Vec<u8>>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                    .map_err(|e| {
                        StorageError::Migration(format!("Query field_validations: {}", e))
                    })?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| {
                        StorageError::Migration(format!("Collect field_validations: {}", e))
                    })?;

                for (id, fv_enc, sig_enc) in &rows {
                    if let Some(enc) = fv_enc {
                        if !enc.is_empty() {
                            let plain = decrypt(old_key, enc).map_err(|e| {
                                StorageError::Migration(format!(
                                    "Decrypt field_value {}: {}",
                                    id, e
                                ))
                            })?;
                            let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                                StorageError::Migration(format!(
                                    "Encrypt field_value {}: {}",
                                    id, e
                                ))
                            })?;
                            self.conn.execute(
                                "UPDATE field_validations SET field_value_encrypted = ?1 WHERE id = ?2",
                                params![new_enc, id],
                            ).map_err(|e| StorageError::Migration(format!("Update field_value {}: {}", id, e)))?;
                        }
                    }
                    if let Some(enc) = sig_enc {
                        if !enc.is_empty() {
                            let plain = decrypt(old_key, enc).map_err(|e| {
                                StorageError::Migration(format!("Decrypt signature {}: {}", id, e))
                            })?;
                            let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                                StorageError::Migration(format!("Encrypt signature {}: {}", id, e))
                            })?;
                            self.conn.execute(
                                "UPDATE field_validations SET signature_encrypted = ?1 WHERE id = ?2",
                                params![new_enc, id],
                            ).map_err(|e| StorageError::Migration(format!("Update signature {}: {}", id, e)))?;
                        }
                    }
                }
            }
            report(&mut completed, "field_validations");

            // Re-encrypt ux_state: aha_tracker_json_encrypted, demo_contact_json_encrypted
            {
                let result = self.conn.query_row(
                    "SELECT id, aha_tracker_json_encrypted, demo_contact_json_encrypted FROM ux_state WHERE id = 1",
                    [],
                    |row| {
                        let id: i64 = row.get(0)?;
                        let aha: Option<Vec<u8>> = row.get(1)?;
                        let demo: Option<Vec<u8>> = row.get(2)?;
                        Ok((id, aha, demo))
                    },
                );

                if let Ok((id, aha_enc, demo_enc)) = result {
                    if let Some(enc) = aha_enc {
                        if !enc.is_empty() {
                            let plain = decrypt(old_key, &enc).map_err(|e| {
                                StorageError::Migration(format!("Decrypt aha_tracker: {}", e))
                            })?;
                            let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                                StorageError::Migration(format!("Encrypt aha_tracker: {}", e))
                            })?;
                            self.conn.execute(
                                "UPDATE ux_state SET aha_tracker_json_encrypted = ?1 WHERE id = ?2",
                                params![new_enc, id],
                            ).map_err(|e| StorageError::Migration(format!("Update aha_tracker: {}", e)))?;
                        }
                    }
                    if let Some(enc) = demo_enc {
                        if !enc.is_empty() {
                            let plain = decrypt(old_key, &enc).map_err(|e| {
                                StorageError::Migration(format!("Decrypt demo_contact: {}", e))
                            })?;
                            let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                                StorageError::Migration(format!("Encrypt demo_contact: {}", e))
                            })?;
                            self.conn.execute(
                                "UPDATE ux_state SET demo_contact_json_encrypted = ?1 WHERE id = ?2",
                                params![new_enc, id],
                            ).map_err(|e| StorageError::Migration(format!("Update demo_contact: {}", e)))?;
                        }
                    }
                }
            }
            report(&mut completed, "ux_state");

            // Re-encrypt audit_log: details_encrypted
            {
                let mut stmt = self.conn
                    .prepare("SELECT id, details_encrypted FROM audit_log WHERE details_encrypted IS NOT NULL")
                    .map_err(|e| StorageError::Migration(format!("Read audit_log for rekey: {}", e)))?;
                let rows: Vec<(i64, Vec<u8>)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .map_err(|e| StorageError::Migration(format!("Query audit_log: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| StorageError::Migration(format!("Collect audit_log: {}", e)))?;

                for (id, enc) in &rows {
                    if !enc.is_empty() {
                        let plain = decrypt(old_key, enc).map_err(|e| {
                            StorageError::Migration(format!("Decrypt audit_log {}: {}", id, e))
                        })?;
                        let new_enc = encrypt(&new_key, &plain).map_err(|e| {
                            StorageError::Migration(format!("Encrypt audit_log {}: {}", id, e))
                        })?;
                        self.conn
                            .execute(
                                "UPDATE audit_log SET details_encrypted = ?1 WHERE id = ?2",
                                params![new_enc, id],
                            )
                            .map_err(|e| {
                                StorageError::Migration(format!("Update audit_log {}: {}", id, e))
                            })?;
                    }
                }
            }
            report(&mut completed, "audit_log");

            Ok(())
        })();

        match result {
            Ok(()) => {
                self.conn.execute_batch("COMMIT")?;
                self.encryption_key = new_key;
                Ok(())
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Begins a database transaction.
    ///
    /// Must be paired with `commit()` or `rollback()`.
    /// Use `BEGIN IMMEDIATE` to acquire a write lock immediately,
    /// preventing deadlocks when multiple writes are planned.
    pub fn begin_transaction(&self) -> Result<(), StorageError> {
        self.conn
            .execute_batch("BEGIN IMMEDIATE TRANSACTION")
            .map_err(StorageError::from)
    }

    /// Commits the current transaction.
    pub fn commit(&self) -> Result<(), StorageError> {
        self.conn
            .execute_batch("COMMIT")
            .map_err(StorageError::from)
    }

    /// Rolls back the current transaction.
    pub fn rollback(&self) {
        let _ = self.conn.execute_batch("ROLLBACK");
    }

    /// Returns a reference to the underlying connection (testing only).
    #[cfg(feature = "testing")]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Returns a reference to the storage encryption key (testing only).
    #[cfg(feature = "testing")]
    pub fn key(&self) -> &SymmetricKey {
        &self.encryption_key
    }
}
