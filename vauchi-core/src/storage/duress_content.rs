// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The duress content as one sealed blob (ADR-032, vauchi/private#1003).
//!
//! Decoy contacts and the duress session's groups, tags and places live in
//! an in-memory database with the tables they had in the file up to v93.
//! The file holds only `duress_content`, one row every database has: the
//! rows of those tables, sealed under the mask key and padded to a size
//! bucket, so a reader without the SMK cannot tell whether there are any.
//! With no content the row seals empty tables, the same way.
//!
//! The mask key is derived from the SEK, one way, so the mask profile can be
//! handed its key without the one that opens the real database (ADR-080).
//!
//! The rows travel as a dump of SQLite values rather than as a serialized
//! database: `Connection::deserialize` needs `unsafe`, which core forbids.

use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::crypto::{SymmetricKey, decrypt, encrypt, kdf::HKDF};

use super::mask::{MaskStorage, SealedSlot};
use super::{Storage, StorageError};

/// The five tables as of v93. A change to one is a change here and a
/// migration that re-seals the blob.
const DURESS_TABLES_SCHEMA: &str = "
    CREATE TABLE decoy_contacts (
        id TEXT PRIMARY KEY,
        card_encrypted BLOB NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        display_name_encrypted BLOB
    );
    CREATE TABLE duress_tags (
        id TEXT PRIMARY KEY,
        name_encrypted BLOB NOT NULL,
        contact_ids_json TEXT NOT NULL DEFAULT '[]',
        created_at INTEGER NOT NULL
    );
    CREATE TABLE duress_places (
        id TEXT PRIMARY KEY,
        data_encrypted BLOB NOT NULL,
        created_at INTEGER NOT NULL
    );
    CREATE TABLE duress_visibility_labels (
        id TEXT PRIMARY KEY,
        name_encrypted BLOB,
        name_hmac BLOB,
        contacts_json_encrypted BLOB,
        visible_fields_json_encrypted BLOB,
        display_name_override_encrypted BLOB,
        bio_override_encrypted BLOB,
        avatar_override_encrypted BLOB,
        created_at INTEGER NOT NULL,
        modified_at INTEGER NOT NULL
    );
    CREATE TABLE duress_contact_visibility_overrides (
        contact_id TEXT NOT NULL,
        field_id TEXT NOT NULL,
        is_visible INTEGER NOT NULL,
        PRIMARY KEY (contact_id, field_id)
    );
";

pub(super) const DURESS_TABLES: [&str; 5] = [
    "decoy_contacts",
    "duress_tags",
    "duress_places",
    "duress_visibility_labels",
    "duress_contact_visibility_overrides",
];

const MASK_KEY_INFO: &[u8] = b"Vauchi_Duress_Mask_Key_v1";

/// The key the duress content is sealed under, derived from the SEK.
pub(crate) fn mask_key(storage_key: &SymmetricKey) -> SymmetricKey {
    let derived = HKDF::derive_key(None, storage_key.as_bytes(), MASK_KEY_INFO);
    SymmetricKey::from_bytes(*derived)
}

/// The smallest sealed size; larger content doubles it until it fits.
const BASE_BUCKET: usize = 4096;
const LENGTH_PREFIX: usize = 4;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum Cell {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct TableRows {
    name: String,
    columns: Vec<String>,
    rows: Vec<Vec<Cell>>,
}

/// An in-memory database holding the five tables, empty.
pub(super) fn open_duress_db() -> Result<Connection, StorageError> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(DURESS_TABLES_SCHEMA)?;
    Ok(conn)
}

/// The sealed size for content of `len` framed bytes.
pub(crate) fn bucket_for(len: usize) -> usize {
    let mut bucket = BASE_BUCKET;
    while bucket < len {
        bucket = bucket.saturating_mul(2);
    }
    bucket
}

/// Every row of the five tables on `conn`, encoded.
pub(super) fn dump(conn: &Connection) -> Result<Vec<u8>, StorageError> {
    let mut tables = Vec::with_capacity(DURESS_TABLES.len());
    for name in DURESS_TABLES {
        let mut stmt = conn.prepare(&format!("SELECT * FROM {name} ORDER BY rowid"))?;
        let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
        let width = columns.len();
        let rows = stmt
            .query_map([], |row| {
                (0..width)
                    .map(|i| {
                        Ok(match row.get::<_, Value>(i)? {
                            Value::Null => Cell::Null,
                            Value::Integer(n) => Cell::Integer(n),
                            Value::Real(r) => Cell::Real(r),
                            Value::Text(t) => Cell::Text(t),
                            Value::Blob(b) => Cell::Blob(b),
                        })
                    })
                    .collect::<rusqlite::Result<Vec<Cell>>>()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        tables.push(TableRows {
            name: name.to_string(),
            columns,
            rows,
        });
    }
    postcard::to_allocvec(&tables).map_err(|e| StorageError::Serialization(e.to_string()))
}

/// Replaces the rows of the five tables on `conn` with `bytes` from [`dump`].
pub(super) fn load_dump(conn: &Connection, bytes: &[u8]) -> Result<(), StorageError> {
    let tables: Vec<TableRows> =
        postcard::from_bytes(bytes).map_err(|e| StorageError::Serialization(e.to_string()))?;
    for name in DURESS_TABLES {
        conn.execute(&format!("DELETE FROM {name}"), [])?;
    }
    for table in tables {
        let Some(name) = DURESS_TABLES.iter().find(|known| **known == table.name) else {
            return Err(StorageError::Serialization(format!(
                "duress content names an unknown table {}",
                table.name
            )));
        };
        let known_columns = table_columns(conn, name)?;
        if let Some(unknown) = table.columns.iter().find(|c| !known_columns.contains(c)) {
            return Err(StorageError::Serialization(format!(
                "duress content names an unknown column {name}.{unknown}"
            )));
        }
        let placeholders = vec!["?"; table.columns.len()].join(", ");
        let insert = format!(
            "INSERT INTO {name} ({}) VALUES ({placeholders})",
            table.columns.join(", ")
        );
        let mut stmt = conn.prepare(&insert)?;
        for row in table.rows {
            if row.len() != table.columns.len() {
                return Err(StorageError::Serialization(format!(
                    "a duress content row of {name} has {} values for {} columns",
                    row.len(),
                    table.columns.len()
                )));
            }
            stmt.execute(params_from_iter(row.into_iter().map(|cell| match cell {
                Cell::Null => Value::Null,
                Cell::Integer(n) => Value::Integer(n),
                Cell::Real(r) => Value::Real(r),
                Cell::Text(t) => Value::Text(t),
                Cell::Blob(b) => Value::Blob(b),
            })))?;
        }
    }
    Ok(())
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, StorageError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(columns)
}

/// `Enc_mask(len || dump || zeros)`, padded to a bucket.
pub(super) fn seal(
    mask_key: &SymmetricKey,
    duress_db: &Connection,
) -> Result<Vec<u8>, StorageError> {
    let body = dump(duress_db)?;
    let len = u32::try_from(body.len())
        .map_err(|_| StorageError::Serialization("duress content over 4 GiB".into()))?;
    let mut framed = Vec::with_capacity(bucket_for(LENGTH_PREFIX + body.len()));
    framed.extend_from_slice(&len.to_le_bytes());
    framed.extend_from_slice(&body);
    framed.resize(bucket_for(framed.len()), 0);
    encrypt(mask_key, &framed).map_err(|e| StorageError::Encryption(e.to_string()))
}

/// Opens `sealed` and loads its rows into `duress_db`.
pub(super) fn unseal_into(
    mask_key: &SymmetricKey,
    sealed: &[u8],
    duress_db: &Connection,
) -> Result<(), StorageError> {
    let framed = decrypt(mask_key, sealed).map_err(|e| StorageError::Encryption(e.to_string()))?;
    let (prefix, rest) = framed.split_first_chunk::<LENGTH_PREFIX>().ok_or_else(|| {
        StorageError::Serialization("duress content shorter than its length".into())
    })?;
    let len = usize::try_from(u32::from_le_bytes(*prefix))
        .map_err(|_| StorageError::Serialization("duress content length".into()))?;
    let body = rest.get(..len).ok_or_else(|| {
        StorageError::Serialization("duress content shorter than its length".into())
    })?;
    load_dump(duress_db, body)
}

/// Seals `duress_db` into the file's `duress_content` row under `key`.
pub(crate) fn write_sealed(
    conn: &Connection,
    duress_db: &Connection,
    key: &SymmetricKey,
) -> Result<(), StorageError> {
    SealedSlot::of(conn).write(&seal(key, duress_db)?)
}

/// Re-seals the blob when a store that holds it has changed the duress
/// database, so every write path persists without naming it.
pub(crate) struct SealOnDrop<'a> {
    slot: SealedSlot<'a>,
    mask: &'a MaskStorage,
    changes_before: std::cell::Cell<u64>,
}

impl<'a> SealOnDrop<'a> {
    pub(crate) fn new(storage: &'a Storage) -> Self {
        SealOnDrop {
            slot: SealedSlot::of(&storage.conn),
            mask: &storage.mask,
            changes_before: std::cell::Cell::new(storage.mask.conn().total_changes()),
        }
    }
}

impl SealOnDrop<'_> {
    /// Seals now and returns the error, for a store's write paths.
    pub(crate) fn seal_now(&self) -> Result<(), StorageError> {
        self.mask.seal_into(self.slot)?;
        self.changes_before.set(self.mask.conn().total_changes());
        Ok(())
    }
}

/// A backstop for a write that did not seal through [`SealOnDrop::seal_now`].
impl Drop for SealOnDrop<'_> {
    fn drop(&mut self) {
        if self.mask.conn().total_changes() == self.changes_before.get() {
            return;
        }
        if let Err(error) = self.mask.seal_into(self.slot) {
            tracing::error!(%error, "[Storage] sealing the duress content failed");
        }
    }
}

impl Storage {
    /// Loads the duress content from the file, writing an empty seal first
    /// when there is none.
    pub(super) fn load_duress_content(&self) -> Result<(), StorageError> {
        let slot = SealedSlot::of(&self.conn);
        let sealed = slot.read()?;
        let Some(sealed) = sealed else {
            return self.mask.seal_into(slot);
        };
        let Err(under_mask_key) = unseal_into(self.mask.key(), &sealed, self.mask.conn()) else {
            return Ok(());
        };
        if unseal_into(&self.encryption_key, &sealed, self.mask.conn()).is_err() {
            return Err(under_mask_key);
        }
        self.move_duress_content_to_mask_key()
    }

    /// A seal that opens under the SEK and not the mask key was written
    /// before the mask key (ADR-080): its fields move to the mask key and it
    /// is sealed again. A seal that opens under neither is refused above.
    fn move_duress_content_to_mask_key(&self) -> Result<(), StorageError> {
        self.mask.conn().execute_batch("BEGIN")?;
        let moved = self
            .rekey_duress_tables(&self.encryption_key, self.mask.key())
            .and_then(|()| self.mask.seal_into(SealedSlot::of(&self.conn)));
        match moved {
            Ok(()) => Ok(self.mask.conn().execute_batch("COMMIT")?),
            Err(e) => {
                // best-effort: the seal in the file was not replaced, so the
                // next open moves it again from the same state
                #[allow(clippy::let_underscore_must_use)]
                let _ = self.mask.conn().execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }
}
