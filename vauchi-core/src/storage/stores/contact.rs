// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact domain persistence view (contacts table + relationship-scoped data).
//!
//! The FK hub. Aggregate deletion stays a cross-cutting orchestrator on
//! `Storage::delete_contact`. Part of problem record
//! `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).

use crate::crypto::SymmetricKey;
use std::sync::Arc;

use rusqlite::{Connection, params};

use super::super::{Storage, StorageError};
use crate::clock::Clock;

use super::contact_row::{CONTACT_COLUMNS, ContactRow};
use crate::contact::Contact;

/// Scoped persistence view for the contact domain.
pub struct ContactStore<'a> {
    pub(super) conn: &'a Connection,
    pub(super) key: &'a SymmetricKey,
    pub(super) clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for the contact domain.
    pub fn contacts(&self) -> ContactStore<'_> {
        ContactStore {
            conn: &self.conn,
            key: &self.encryption_key,
            clock: &self.clock,
        }
    }
}

impl ContactStore<'_> {
    pub(super) fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }
    /// Saves a contact to storage.
    ///
    /// If the contact has a CEK, the card is encrypted with the CEK and the
    /// CEK itself with the storage key (`cek_encrypted`); otherwise the card
    /// is encrypted with the storage key. The name lives only in the card.
    pub fn save_contact(&self, contact: &Contact) -> Result<(), StorageError> {
        let row = self.contact_to_row(contact)?;

        // Upsert (not INSERT OR REPLACE which cascades deletes to field_notes)
        self.conn.execute(
            "INSERT INTO contacts
             (id, public_key, card_encrypted, shared_key_encrypted,
              visibility_rules_encrypted, exchange_timestamp, fingerprint_verified, last_sync_at,
              blocked, hidden, favorite, recovery_trusted, proposal_trusted, cek_encrypted,
              exchange_transport, has_recovered, card_updated_at,
              relay_url, trust_metrics,
              contact_kind, import_source, imported_at, original_uid,
              deleted_at, archived, archived_at, ignored, ignored_at,
              reciprocity, confirmation_channel)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30)
             ON CONFLICT(id) DO UPDATE SET
               public_key              = excluded.public_key,
               card_encrypted          = excluded.card_encrypted,
               shared_key_encrypted    = excluded.shared_key_encrypted,
               visibility_rules_encrypted = excluded.visibility_rules_encrypted,
               exchange_timestamp      = excluded.exchange_timestamp,
               fingerprint_verified    = excluded.fingerprint_verified,
               blocked                 = excluded.blocked,
               hidden                  = excluded.hidden,
               favorite                = excluded.favorite,
               recovery_trusted        = excluded.recovery_trusted,
               proposal_trusted        = excluded.proposal_trusted,
               cek_encrypted           = excluded.cek_encrypted,
               exchange_transport      = excluded.exchange_transport,
               has_recovered           = excluded.has_recovered,
               card_updated_at         = excluded.card_updated_at,
               relay_url               = excluded.relay_url,
               trust_metrics           = excluded.trust_metrics,
               contact_kind            = excluded.contact_kind,
               import_source           = excluded.import_source,
               imported_at             = excluded.imported_at,
               original_uid            = excluded.original_uid,
               deleted_at              = excluded.deleted_at,
               archived                = excluded.archived,
               archived_at             = excluded.archived_at,
               ignored                 = excluded.ignored,
               ignored_at              = excluded.ignored_at,
               reciprocity             = excluded.reciprocity,
               confirmation_channel    = excluded.confirmation_channel",
            params![
                row.id,
                row.public_key,
                row.card_encrypted,
                row.shared_key_encrypted,
                row.visibility_rules_encrypted,
                row.exchange_timestamp,
                row.fingerprint_verified,
                Option::<i64>::None,
                row.blocked,
                row.hidden,
                row.favorite,
                row.recovery_trusted,
                row.proposal_trusted,
                row.cek_encrypted,
                row.exchange_transport,
                row.has_recovered,
                row.card_updated_at,
                row.relay_url,
                row.trust_metrics,
                row.contact_kind,
                row.import_source,
                row.imported_at,
                row.original_uid,
                row.deleted_at,
                row.archived,
                row.archived_at,
                row.ignored,
                row.ignored_at,
                row.reciprocity,
                row.confirmation_channel,
            ],
        )?;

        Ok(())
    }
    /// Loads a contact by ID.
    pub fn load_contact(&self, id: &str) -> Result<Option<Contact>, StorageError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {CONTACT_COLUMNS} FROM contacts WHERE id = ?1"
        ))?;

        let result = stmt.query_row(params![id], ContactRow::from_row);

        match result {
            Ok(row) => Ok(Some(self.row_to_contact(row)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
    /// Lists all contacts, excluding soft-deleted and archived contacts.
    pub fn list_contacts(&self) -> Result<Vec<Contact>, StorageError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {CONTACT_COLUMNS} FROM contacts
             WHERE deleted_at IS NULL AND archived = 0"
        ))?;

        let rows = stmt.query_map([], ContactRow::from_row)?;

        let mut contacts = Vec::new();
        let mut row_count = 0usize;
        for row_result in rows {
            let row = row_result?;
            row_count += 1;
            // Dev instrumentation (no PII — counts/errors only). Distinguishes a
            // query miss from a decrypt failure: a single row that fails to
            // materialize (`?`) blanks the WHOLE list, so a saved-but-
            // undecryptable contact reads as "no contacts" (2026-07-25 S7
            // exchanged-contact-not-listed investigation).
            match self.row_to_contact(row) {
                Ok(c) => contacts.push(c),
                Err(e) => {
                    tracing::warn!(
                        "[MSX] list_contacts: row {row_count} failed to materialize \
                         ({e:?}) — the whole list blanks"
                    );
                    return Err(e);
                }
            }
        }
        tracing::info!(
            "[MSX] list_contacts: {row_count} rows -> {} contacts",
            contacts.len()
        );

        sort_by_display_name(&mut contacts);
        Ok(contacts)
    }
    /// Returns true if at least one active contact exists, excluding
    /// soft-deleted and archived contacts (same filter as
    /// `list_contacts`). O(1) existence check for UI gating that runs
    /// on every screen emit.
    pub fn has_contacts(&self) -> Result<bool, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM contacts WHERE deleted_at IS NULL AND archived = 0 LIMIT 1")?;
        Ok(stmt.exists([])?)
    }
    /// Lists contacts with pagination support.
    ///
    /// Returns contacts ordered by display name, starting from `offset`
    /// and returning at most `limit` results. Names are only readable after
    /// decryption, so the page is cut from the full sorted list (#570).
    pub fn list_contacts_paginated(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Contact>, StorageError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Ok(self
            .list_contacts()?
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect())
    }
    /// Searches contacts by display name using case-insensitive matching.
    ///
    /// Returns all contacts whose display name contains the query string,
    /// in display-name order. An empty query returns all contacts.
    pub fn search_contacts(&self, query: &str) -> Result<Vec<Contact>, StorageError> {
        let query_lower = query.to_lowercase();
        let mut contacts = self.list_contacts()?;
        contacts.retain(|contact| contact.display_name().to_lowercase().contains(&query_lower));
        Ok(contacts)
    }
    /// Lists contacts that are archived (but not soft-deleted).
    pub fn list_archived_contacts(&self) -> Result<Vec<Contact>, StorageError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {CONTACT_COLUMNS} FROM contacts
             WHERE archived = 1 AND deleted_at IS NULL"
        ))?;

        let rows = stmt.query_map([], ContactRow::from_row)?;

        let mut contacts = Vec::new();
        for row_result in rows {
            let row = row_result?;
            contacts.push(self.row_to_contact(row)?);
        }

        sort_by_display_name(&mut contacts);
        Ok(contacts)
    }
    /// Finds contact IDs that were soft-deleted before the given timestamp.
    ///
    /// Used by the garbage collector to find contacts eligible for permanent deletion.
    pub fn find_stale_soft_deletes(&self, older_than: u64) -> Result<Vec<String>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM contacts WHERE deleted_at IS NOT NULL AND deleted_at < ?1")?;

        let rows = stmt.query_map(params![older_than as i64], |row| row.get::<_, String>(0))?;

        let mut ids = Vec::new();
        for row_result in rows {
            ids.push(row_result?);
        }

        Ok(ids)
    }
    /// Finds an imported contact by its original UID.
    ///
    /// Returns `Some(contact_id)` if a contact with the given UID exists,
    /// `None` otherwise. Only searches imported contacts (`contact_kind = 'imported'`).
    pub fn find_imported_by_uid(&self, uid: &str) -> Result<Option<String>, StorageError> {
        let result = self.conn.query_row(
            "SELECT id FROM contacts WHERE original_uid = ?1 AND contact_kind = 'imported' LIMIT 1",
            params![uid],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
    /// List contacts with a specific reciprocity status (e.g., "pending").
    ///
    /// Used by the relaunch recovery scan to find contacts whose
    /// reciprocity confirmation cascade should be resumed or expired.
    pub fn list_contacts_by_reciprocity(
        &self,
        reciprocity: &str,
    ) -> Result<Vec<Contact>, StorageError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {CONTACT_COLUMNS} FROM contacts
             WHERE reciprocity = ?1 AND deleted_at IS NULL"
        ))?;

        let rows = stmt.query_map(params![reciprocity], ContactRow::from_row)?;

        let mut contacts = Vec::new();
        for row_result in rows {
            let row = row_result?;
            contacts.push(self.row_to_contact(row)?);
        }
        Ok(contacts)
    }
    /// Save encrypted confirmation state for crash recovery (design spec §5.1).
    pub fn update_confirmation_state(
        &self,
        contact_id: &str,
        state_bytes: &[u8],
    ) -> Result<(), StorageError> {
        let encrypted = crate::crypto::encrypt(self.key, state_bytes)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            "UPDATE contacts SET confirmation_state = ?1 WHERE id = ?2",
            params![encrypted, contact_id],
        )?;
        Ok(())
    }
    /// Load and decrypt confirmation state for crash recovery.
    pub fn load_confirmation_state(
        &self,
        contact_id: &str,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        let result: Result<Option<Vec<u8>>, _> = self.conn.query_row(
            "SELECT confirmation_state FROM contacts WHERE id = ?1",
            params![contact_id],
            |row| row.get(0),
        );
        match result {
            Ok(Some(encrypted)) => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                Ok(Some(decrypted))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
    /// Test-only: overwrite a single TEXT column on the `contacts` row
    /// for the given contact_id. Used to inject deserialization-failure
    /// inputs for the trust-input columns (site 8 of
    /// `2026-05-21-silent-failures-in-security-paths`): the public
    /// setters refuse garbage by type, so direct SQL is the only way
    /// to reach the parser in [`Storage::row_to_contact`].
    ///
    /// The column name is interpolated into the SQL string with an
    /// allow-list check so the helper cannot be turned into a SQL
    /// injection vector by a wandering caller.
    #[cfg(any(test, feature = "testing"))]
    pub fn test_corrupt_contact_text_column(
        &self,
        contact_id: &str,
        column: &str,
        value: &str,
    ) -> Result<(), StorageError> {
        const ALLOWED: &[&str] = &[
            "exchange_transport",
            "trust_metrics",
            "reciprocity",
            "confirmation_channel",
        ];
        if !ALLOWED.contains(&column) {
            return Err(StorageError::InvalidData(format!(
                "test_corrupt_contact_text_column: column {column} not in allow-list"
            )));
        }
        let sql = format!("UPDATE contacts SET {column} = ?1 WHERE id = ?2");
        self.conn.execute(&sql, params![value, contact_id])?;
        Ok(())
    }
}

/// Orders contacts by decrypted display name, case-insensitively; names
/// are not stored in plaintext, so SQL cannot sort them (#570).
fn sort_by_display_name(contacts: &mut [Contact]) {
    contacts.sort_by_cached_key(|contact| {
        (
            contact.display_name().to_lowercase(),
            contact.id().to_string(),
        )
    });
}
