// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The mask profile's database: the regular profile schema, in memory,
//! under the mask key, sealed into the real file's one `duress_content` row
//! (ADR-080 point 4, amended 2026-11-08). A real `Storage` embeds one for
//! the duress twins; [`Storage::open_mask_profile`] opens one on its own, for
//! the duress foreground, holding no storage key.

use std::path::Path;

use rusqlite::Connection;

use super::duress_content::{LEGACY_TABLES, LegacyContent, open_seal, seal_body};
use super::mask::SealedSlot;
use super::{Storage, StorageError};
use crate::crypto::SymmetricKey;

/// What only a mask profile has beside the regular schema: the mask seed,
/// and the decoy ids its contacts were added under, which the decoy API is
/// keyed by.
const MASK_ONLY_SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS mask_seed (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        seed_encrypted BLOB NOT NULL
    );
    CREATE TABLE IF NOT EXISTS public_values (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        signing_public_key_encrypted BLOB NOT NULL,
        display_name_encrypted BLOB NOT NULL
    );
    CREATE TABLE IF NOT EXISTS decoy_ids (
        contact_id TEXT PRIMARY KEY,
        decoy_id TEXT NOT NULL UNIQUE,
        display_name_encrypted BLOB NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
";

/// Tables a profile database has that never belong in a seal.
const NEVER_SEALED: [&str; 2] = ["schema_version", "duress_content"];

impl Storage {
    /// The mask profile: the one a real database embeds, or this one.
    pub(crate) fn mask_profile(&self) -> &Storage {
        self.mask.as_deref().unwrap_or(self)
    }

    /// The key the mask profile's fields are under.
    pub(crate) fn mask_key(&self) -> &SymmetricKey {
        &self.mask_profile().encryption_key
    }

    /// Seals the mask profile into its row: the real file's slot for an
    /// embedded one, its own seal target otherwise.
    pub(crate) fn seal_mask(&self) -> Result<(), StorageError> {
        match &self.mask {
            Some(mask) => SealedSlot::of(&self.conn).write(&mask.sealed_mask()?),
            None => self.reseal(),
        }
    }

    /// `store`, from the mask profile, resealing into this database's row
    /// when it drops after a write.
    pub(crate) fn mask_sealed<'a, S>(
        &'a self,
        store: super::Sealed<'a, S>,
    ) -> super::Sealed<'a, S> {
        match &self.mask {
            Some(mask) => store.resealed_by(mask, self),
            None => store,
        }
    }

    /// An empty mask profile under `mask_key`, sealed into `seal_target`'s
    /// row when it has one.
    pub(super) fn empty_mask_profile(
        mask_key: SymmetricKey,
        seal_target: Option<Connection>,
    ) -> Result<Storage, StorageError> {
        let conn = Connection::open_in_memory()?;
        Self::configure_pragmas(&conn)?;
        let storage = Storage {
            conn,
            mask: None,
            encryption_key: mask_key,
            seal_target,
            receive_key: None,
            db_path: None,
            clock: crate::clock::SystemClock::shared(),
            #[cfg(any(test, feature = "testing"))]
            commit_fault: std::cell::Cell::new(false),
        };
        super::migration::MigrationRunner::run(
            &storage.conn,
            &storage.encryption_key,
            super::migration::all_migrations(),
            None,
            storage.clock.unix_seconds(),
        )?;
        storage.conn.execute_batch(MASK_ONLY_SCHEMA)?;
        Ok(storage)
    }

    /// Opens the mask profile sealed in the database at `path`, under
    /// `mask_key`. It holds the file only through the sealed row and never
    /// the storage key; a key that does not open the seal is refused.
    pub fn open_mask_profile(path: &Path, mask_key: SymmetricKey) -> Result<Storage, StorageError> {
        let file = Connection::open(path)?;
        Self::configure_pragmas(&file)?;
        let sealed = SealedSlot::of(&file).read()?;
        let mask = Self::empty_mask_profile(mask_key, Some(file))?;
        if let Some(sealed) = sealed {
            let body = open_seal(&mask.encryption_key, &sealed)?;
            if mask.load_body(&body, mask.mask_key())? {
                mask.reseal()?;
            }
        }
        Ok(mask)
    }

    /// Loads a seal's `body` into this mask profile. A body in the legacy
    /// decoy-table shape, whose fields are under `fields_key`, converts into
    /// the regular tables; returns whether it did, so the caller seals the
    /// new shape.
    pub(super) fn load_body(
        &self,
        body: &[u8],
        fields_key: &SymmetricKey,
    ) -> Result<bool, StorageError> {
        let tables = super::duress_content::decode_tables(body)?;
        if tables
            .iter()
            .any(|table| LEGACY_TABLES.contains(&table.name.as_str()))
        {
            let legacy = LegacyContent::load(tables)?;
            legacy.convert_into(self, fields_key)?;
            return Ok(true);
        }
        super::duress_content::insert_tables(&self.conn, tables)?;
        Ok(false)
    }

    /// The mask profile's rows, as a seal's body.
    pub(super) fn sealed_body(&self) -> Result<Vec<u8>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table'
             AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut with_rows = Vec::new();
        for name in names {
            if NEVER_SEALED.contains(&name.as_str()) {
                continue;
            }
            let rows: i64 =
                self.conn
                    .query_row(&format!("SELECT COUNT(*) FROM {name}"), [], |row| {
                        row.get(0)
                    })?;
            if rows > 0 {
                with_rows.push(name);
            }
        }
        let names: Vec<&str> = with_rows.iter().map(String::as_str).collect();
        super::duress_content::dump_tables(&self.conn, &names)
    }

    /// This mask profile, sealed under its key.
    pub(super) fn sealed_mask(&self) -> Result<Vec<u8>, StorageError> {
        seal_body(&self.encryption_key, &self.sealed_body()?)
    }
}
