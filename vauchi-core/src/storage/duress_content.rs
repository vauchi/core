// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The mask profile as one sealed row of the real file (ADR-032,
//! vauchi/private#1003; ADR-080).
//!
//! The file holds only `duress_content`, one row every database has: the
//! mask profile's rows, sealed under the mask key and padded to a size
//! bucket, so a reader without the SMK cannot tell whether there are any.
//!
//! The mask key is derived from the SEK, one way, so the mask profile can be
//! handed its key without the one that opens the real database (ADR-080).
//!
//! Up to 2026-11-08 the seal held twin tables of their own (`decoy_contacts`,
//! `duress_tags` ...), the legacy shape below; migrations v94 and v114 still
//! write it, and a seal in it converts into the regular schema when it loads
//! (`mask_profile.rs`).
//!
//! The rows travel as a dump of SQLite values rather than as a serialized
//! database: `Connection::deserialize` needs `unsafe`, which core forbids.

use rusqlite::types::Value;
use rusqlite::{Connection, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::crypto::{SymmetricKey, decrypt, encrypt, kdf::HKDF};

use super::mask::SealedSlot;
use super::{Storage, StorageError};

/// The legacy shape's tables, as v93 to 2026-11-08 sealed them.
const LEGACY_SCHEMA: &str = "
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
    CREATE TABLE duress_own_card (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        card_json TEXT NOT NULL,
        card_json_encrypted BLOB,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE duress_emergency_config (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        trusted_contact_ids_encrypted BLOB,
        message_encrypted BLOB,
        include_location INTEGER DEFAULT 0,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE mask_seed (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        seed_encrypted BLOB NOT NULL
    );
";

/// The tables only the legacy shape has; a seal naming one is legacy.
pub(super) const LEGACY_TABLES: [&str; 7] = [
    "decoy_contacts",
    "duress_tags",
    "duress_places",
    "duress_visibility_labels",
    "duress_contact_visibility_overrides",
    "duress_own_card",
    "duress_emergency_config",
];

/// Each legacy twin and the regular table that replaces it, with the same
/// columns: the twin stores ran the regular SQL on another table name.
const LEGACY_TWINS: [(&str, &str); 6] = [
    ("duress_tags", "tags"),
    ("duress_places", "places"),
    ("duress_visibility_labels", "visibility_labels"),
    (
        "duress_contact_visibility_overrides",
        "contact_visibility_overrides",
    ),
    ("duress_own_card", "own_card"),
    ("duress_emergency_config", "emergency_config"),
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
pub(super) struct TableRows {
    pub(super) name: String,
    columns: Vec<String>,
    rows: Vec<Vec<Cell>>,
}

/// An in-memory database in the legacy shape, empty.
pub(super) fn open_duress_db() -> Result<Connection, StorageError> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(LEGACY_SCHEMA)?;
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

/// Every row of `names` on `conn`, encoded.
pub(super) fn dump_tables(conn: &Connection, names: &[&str]) -> Result<Vec<u8>, StorageError> {
    let mut tables = Vec::with_capacity(names.len());
    for name in names {
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

pub(super) fn decode_tables(bytes: &[u8]) -> Result<Vec<TableRows>, StorageError> {
    postcard::from_bytes(bytes).map_err(|e| StorageError::Serialization(e.to_string()))
}

/// Inserts `tables`' rows into `conn`, whose tables they must name.
pub(super) fn insert_tables(conn: &Connection, tables: Vec<TableRows>) -> Result<(), StorageError> {
    for table in tables {
        let name = table.name;
        let known_columns = table_columns(conn, &name)?;
        if known_columns.is_empty() {
            return Err(StorageError::Serialization(format!(
                "duress content names an unknown table {name}"
            )));
        }
        if let Some(unknown) = table.columns.iter().find(|c| !known_columns.contains(c)) {
            return Err(StorageError::Serialization(format!(
                "duress content names an unknown column {name}.{unknown}"
            )));
        }
        let placeholders = vec!["?"; table.columns.len()].join(", ");
        let insert = format!(
            "INSERT OR REPLACE INTO {name} ({}) VALUES ({placeholders})",
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

/// Replaces the legacy tables' rows on `conn` with `bytes` from
/// [`dump_tables`].
pub(super) fn load_dump(conn: &Connection, bytes: &[u8]) -> Result<(), StorageError> {
    for name in LEGACY_TABLES.iter().chain(["mask_seed"].iter()) {
        conn.execute(&format!("DELETE FROM {name}"), [])?;
    }
    insert_tables(conn, decode_tables(bytes)?)
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, StorageError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(columns)
}

/// `Enc_key(len || body || zeros)`, padded to a bucket.
pub(super) fn seal_body(key: &SymmetricKey, body: &[u8]) -> Result<Vec<u8>, StorageError> {
    let len = u32::try_from(body.len())
        .map_err(|_| StorageError::Serialization("duress content over 4 GiB".into()))?;
    let mut framed = Vec::with_capacity(bucket_for(LENGTH_PREFIX + body.len()));
    framed.extend_from_slice(&len.to_le_bytes());
    framed.extend_from_slice(body);
    framed.resize(bucket_for(framed.len()), 0);
    encrypt(key, &framed).map_err(|e| StorageError::Encryption(e.to_string()))
}

/// The body sealed in `sealed`, which must open under `key`.
pub(super) fn open_seal(key: &SymmetricKey, sealed: &[u8]) -> Result<Vec<u8>, StorageError> {
    let framed = decrypt(key, sealed).map_err(|e| StorageError::Encryption(e.to_string()))?;
    let (prefix, rest) = framed.split_first_chunk::<LENGTH_PREFIX>().ok_or_else(|| {
        StorageError::Serialization("duress content shorter than its length".into())
    })?;
    let len = usize::try_from(u32::from_le_bytes(*prefix))
        .map_err(|_| StorageError::Serialization("duress content length".into()))?;
    rest.get(..len)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| StorageError::Serialization("duress content shorter than its length".into()))
}

/// Seals a legacy-shape `duress_db` into the file's `duress_content` row
/// under `key`, as migration v94 does.
pub(crate) fn write_sealed(
    conn: &Connection,
    duress_db: &Connection,
    key: &SymmetricKey,
) -> Result<(), StorageError> {
    let mut names: Vec<&str> = LEGACY_TABLES.to_vec();
    names.push("mask_seed");
    SealedSlot::of(conn).write(&seal_body(key, &dump_tables(duress_db, &names)?)?)
}

/// A seal's content in the legacy shape, on its own in-memory database.
pub(super) struct LegacyContent {
    conn: Connection,
}

impl LegacyContent {
    pub(super) fn load(tables: Vec<TableRows>) -> Result<Self, StorageError> {
        let conn = open_duress_db()?;
        insert_tables(&conn, tables)?;
        Ok(LegacyContent { conn })
    }

    /// Moves this content into `mask`'s regular tables. Its fields are under
    /// `fields_key`: the mask key, or the SEK for a seal from before ADR-080.
    pub(super) fn convert_into(
        &self,
        mask: &Storage,
        fields_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        for (twin, regular) in LEGACY_TWINS {
            self.copy_rows(mask, twin, regular)?;
        }
        if !bool::from(subtle::ConstantTimeEq::ct_eq(
            fields_key.as_bytes().as_slice(),
            mask.mask_key().as_bytes().as_slice(),
        )) {
            mask.rekey_mask_twins(fields_key, mask.mask_key())?;
        }
        self.convert_decoys(mask, fields_key)?;
        self.convert_seed(mask, fields_key)
    }

    fn copy_rows(&self, mask: &Storage, twin: &str, regular: &str) -> Result<(), StorageError> {
        let body = dump_tables(&self.conn, &[twin])?;
        let mut tables = decode_tables(&body)?;
        for table in &mut tables {
            table.name = regular.to_string();
        }
        insert_tables(&mask.conn, tables)
    }

    fn convert_decoys(
        &self,
        mask: &Storage,
        fields_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, display_name_encrypted, card_encrypted, created_at, updated_at
             FROM decoy_contacts ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<Vec<u8>>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, name, card, created_at, updated_at) in rows {
            let card =
                decrypt(fields_key, &card).map_err(|e| StorageError::Encryption(e.to_string()))?;
            let card: crate::card::ContactCard = serde_json::from_slice(&card)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            let name = match name {
                Some(name) => String::from_utf8(
                    decrypt(fields_key, &name)
                        .map_err(|e| StorageError::Encryption(e.to_string()))?,
                )
                .map_err(|e| StorageError::Serialization(e.to_string()))?,
                None => card.display_name().to_string(),
            };
            mask.decoy().insert_decoy(
                &id,
                &name,
                &card,
                u64::try_from(created_at).unwrap_or_default(),
                u64::try_from(updated_at).unwrap_or_default(),
            )?;
        }
        Ok(())
    }

    fn convert_seed(&self, mask: &Storage, fields_key: &SymmetricKey) -> Result<(), StorageError> {
        let sealed: Option<Vec<u8>> = rusqlite::OptionalExtension::optional(self.conn.query_row(
            "SELECT seed_encrypted FROM mask_seed WHERE id = 1",
            [],
            |row| row.get(0),
        ))?;
        let Some(sealed) = sealed else {
            return Ok(());
        };
        let seed = zeroize::Zeroizing::new(
            decrypt(fields_key, &sealed).map_err(|e| StorageError::Encryption(e.to_string()))?,
        );
        let under_mask =
            encrypt(mask.mask_key(), &seed).map_err(|e| StorageError::Encryption(e.to_string()))?;
        mask.conn.execute(
            "INSERT OR REPLACE INTO mask_seed (id, seed_encrypted) VALUES (1, ?1)",
            params![under_mask],
        )?;
        Ok(())
    }
}

impl Storage {
    /// Loads the mask profile from the file's seal, writing an empty seal
    /// first when there is none. A seal that opens only under the SEK is
    /// from before the mask key (ADR-080); it converts and is sealed again
    /// under the mask key, as is one in the legacy shape.
    pub(super) fn load_duress_content(&self) -> Result<(), StorageError> {
        let Some(sealed) = SealedSlot::of(&self.conn).read()? else {
            return self.seal_mask();
        };
        let mask = self.mask_profile();
        let resealed = match open_seal(mask.mask_key(), &sealed) {
            Ok(body) => mask.load_body(&body, mask.mask_key())?,
            Err(under_mask_key) => {
                let Ok(body) = open_seal(&self.encryption_key, &sealed) else {
                    return Err(under_mask_key);
                };
                mask.load_body(&body, &self.encryption_key)?;
                true
            }
        };
        if resealed {
            self.seal_mask()?;
        }
        Ok(())
    }
}
