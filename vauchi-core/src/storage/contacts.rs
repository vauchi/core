// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact storage operations.

use rusqlite::params;

use super::contact_row::ContactRow;
use super::{Storage, StorageError};
use crate::contact::Contact;

impl Storage {
    // === Contact Operations ===

    /// Saves a contact to storage.
    ///
    /// If the contact has a CEK, the card is encrypted with the CEK (not the
    /// storage key) and the `display_name` column is set to NULL. The CEK itself
    /// is encrypted with the storage key and stored in `cek_encrypted`.
    ///
    /// Legacy contacts (no CEK) use storage-key encryption with plaintext
    /// display_name (existing behavior).
    pub fn save_contact(&self, contact: &Contact) -> Result<(), StorageError> {
        // Serialize the contact card
        let card_json = serde_json::to_vec(contact.card())
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        // CEK-aware encryption: use CEK if present, otherwise storage key
        let (card_encrypted, display_name_db, cek_encrypted_param) =
            if let Some(cek) = contact.cek() {
                let card_ct = cek
                    .encrypt(&card_json)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let cek_ct = crate::crypto::encrypt(&self.encryption_key, &cek.to_bytes())
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                (card_ct, String::new(), Some(cek_ct))
            } else {
                let card_ct = crate::crypto::encrypt(&self.encryption_key, &card_json)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                (card_ct, contact.display_name().to_string(), None::<Vec<u8>>)
            };

        // Branch on contact kind for crypto fields
        let (
            public_key_bytes,
            shared_key_encrypted,
            visibility_encrypted,
            exchange_timestamp,
            fingerprint_verified,
            recovery_trusted,
            proposal_trusted,
            transport_str,
            has_recovered,
            relay_url,
            relay_noise_pubkey,
            trust_metrics_json,
            contact_kind_str,
            import_source_str,
            imported_at_val,
            original_uid_val,
        ) = if let Some(ex) = contact.kind().exchanged_data() {
            // Exchanged contact: encrypt all crypto fields
            let sk_encrypted =
                crate::crypto::encrypt(&self.encryption_key, ex.shared_key.as_bytes())
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let vis_json = serde_json::to_string(&ex.visibility_rules)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            let vis_encrypted = crate::crypto::encrypt(&self.encryption_key, vis_json.as_bytes())
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let transport = serde_json::to_value(ex.exchange_transport)
                .map_err(|e| StorageError::Serialization(e.to_string()))?
                .as_str()
                .unwrap_or("Qr")
                .to_string();
            let tm_json = ex
                .trust_metrics
                .as_ref()
                .map(|m| {
                    serde_json::to_string(m).map_err(|e| StorageError::Serialization(e.to_string()))
                })
                .transpose()?;

            (
                ex.public_key.to_vec(),
                sk_encrypted,
                Some(vis_encrypted),
                ex.exchange_timestamp as i64,
                ex.fingerprint_verified as i32,
                ex.recovery_trusted as i32,
                ex.proposal_trusted as i32,
                transport,
                ex.has_recovered as i32,
                ex.relay_url.clone(),
                ex.relay_noise_pubkey.map(|k| k.to_vec()),
                tm_json,
                "exchanged".to_string(),
                None::<String>,
                None::<i64>,
                None::<String>,
            )
        } else if let Some(imp) = contact.kind().imported_data() {
            // Imported contact: empty blobs for crypto columns (HR-2 defense-in-depth).
            // If these accidentally reach try_into::<[u8; 32]>(), they fail safely.
            let source_str = serde_json::to_string(&imp.source)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;

            (
                vec![],        // public_key: empty blob
                vec![],        // shared_key_encrypted: empty blob
                None,          // visibility_rules_encrypted: NULL
                0i64,          // exchange_timestamp: 0
                0i32,          // fingerprint_verified: false
                0i32,          // recovery_trusted: false
                0i32,          // proposal_trusted: false
                String::new(), // exchange_transport: empty
                0i32,          // has_recovered: false
                None,          // relay_url: None
                None,          // relay_noise_pubkey: None
                None,          // trust_metrics: None
                "imported".to_string(),
                Some(source_str),
                Some(imp.imported_at as i64),
                imp.original_uid.clone(),
            )
        } else {
            return Err(StorageError::Serialization("Unknown contact kind".into()));
        };

        // Upsert (not INSERT OR REPLACE which cascades deletes to field_notes)
        self.conn.execute(
            "INSERT INTO contacts
             (id, public_key, display_name, card_encrypted, shared_key_encrypted,
              visibility_rules_encrypted, exchange_timestamp, fingerprint_verified, last_sync_at,
              blocked, hidden, favorite, recovery_trusted, proposal_trusted, cek_encrypted,
              exchange_transport, has_recovered, card_updated_at,
              relay_url, relay_noise_pubkey, trust_metrics,
              contact_kind, import_source, imported_at, original_uid,
              deleted_at, archived, archived_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28)
             ON CONFLICT(id) DO UPDATE SET
               public_key              = excluded.public_key,
               display_name            = excluded.display_name,
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
               relay_noise_pubkey      = excluded.relay_noise_pubkey,
               trust_metrics           = excluded.trust_metrics,
               contact_kind            = excluded.contact_kind,
               import_source           = excluded.import_source,
               imported_at             = excluded.imported_at,
               original_uid            = excluded.original_uid,
               deleted_at              = excluded.deleted_at,
               archived                = excluded.archived,
               archived_at             = excluded.archived_at",
            params![
                contact.id(),
                public_key_bytes,
                display_name_db,
                card_encrypted,
                shared_key_encrypted,
                visibility_encrypted,
                exchange_timestamp,
                fingerprint_verified,
                Option::<i64>::None,
                contact.is_blocked() as i32,
                contact.is_hidden() as i32,
                contact.is_favorite() as i32,
                recovery_trusted,
                proposal_trusted,
                cek_encrypted_param,
                transport_str,
                has_recovered,
                contact.card_updated_at().map(|t| t as i64),
                relay_url,
                relay_noise_pubkey,
                trust_metrics_json,
                contact_kind_str,
                import_source_str,
                imported_at_val,
                original_uid_val,
                contact.deleted_at().map(|t| t as i64),
                contact.is_archived() as i32,
                contact.archived_at().map(|t| t as i64),
            ],
        )?;

        Ok(())
    }

    /// Loads a contact by ID.
    pub fn load_contact(&self, id: &str) -> Result<Option<Contact>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, public_key, display_name, card_encrypted, shared_key_encrypted,
                    visibility_rules_json, visibility_rules_encrypted, exchange_timestamp,
                    fingerprint_verified, blocked, hidden, favorite, recovery_trusted,
                    proposal_trusted, cek_encrypted, exchange_transport, has_recovered,
                    card_updated_at, relay_url, relay_noise_pubkey, trust_metrics,
                    contact_kind, import_source, imported_at, original_uid,
                    deleted_at, archived, archived_at
             FROM contacts WHERE id = ?1",
        )?;

        let result = stmt.query_row(params![id], |row| {
            Ok(ContactRow {
                id: row.get(0)?,
                public_key: row.get(1)?,
                display_name: row.get(2)?,
                card_encrypted: row.get(3)?,
                shared_key_encrypted: row.get(4)?,
                visibility_rules_json: row.get(5)?,
                visibility_rules_encrypted: row.get(6)?,
                exchange_timestamp: row.get(7)?,
                fingerprint_verified: row.get(8)?,
                blocked: row.get(9)?,
                hidden: row.get(10)?,
                favorite: row.get(11)?,
                recovery_trusted: row.get(12)?,
                proposal_trusted: row.get(13)?,
                cek_encrypted: row.get(14)?,
                exchange_transport: row.get(15)?,
                has_recovered: row.get(16)?,
                card_updated_at: row.get(17)?,
                relay_url: row.get(18)?,
                relay_noise_pubkey: row.get(19)?,
                trust_metrics: row.get(20)?,
                contact_kind: row.get(21)?,
                import_source: row.get(22)?,
                imported_at: row.get(23)?,
                original_uid: row.get(24)?,
                deleted_at: row.get(25)?,
                archived: row.get(26)?,
                archived_at: row.get(27)?,
            })
        });

        match result {
            Ok(row) => Ok(Some(self.row_to_contact(row)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Lists all contacts, excluding soft-deleted and archived contacts.
    pub fn list_contacts(&self) -> Result<Vec<Contact>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, public_key, display_name, card_encrypted, shared_key_encrypted,
                    visibility_rules_json, visibility_rules_encrypted, exchange_timestamp,
                    fingerprint_verified, blocked, hidden, favorite, recovery_trusted,
                    proposal_trusted, cek_encrypted, exchange_transport, has_recovered,
                    card_updated_at, relay_url, relay_noise_pubkey, trust_metrics,
                    contact_kind, import_source, imported_at, original_uid,
                    deleted_at, archived, archived_at
             FROM contacts
             WHERE deleted_at IS NULL AND archived = 0
             ORDER BY display_name",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(ContactRow {
                id: row.get(0)?,
                public_key: row.get(1)?,
                display_name: row.get(2)?,
                card_encrypted: row.get(3)?,
                shared_key_encrypted: row.get(4)?,
                visibility_rules_json: row.get(5)?,
                visibility_rules_encrypted: row.get(6)?,
                exchange_timestamp: row.get(7)?,
                fingerprint_verified: row.get(8)?,
                blocked: row.get(9)?,
                hidden: row.get(10)?,
                favorite: row.get(11)?,
                recovery_trusted: row.get(12)?,
                proposal_trusted: row.get(13)?,
                cek_encrypted: row.get(14)?,
                exchange_transport: row.get(15)?,
                has_recovered: row.get(16)?,
                card_updated_at: row.get(17)?,
                relay_url: row.get(18)?,
                relay_noise_pubkey: row.get(19)?,
                trust_metrics: row.get(20)?,
                contact_kind: row.get(21)?,
                import_source: row.get(22)?,
                imported_at: row.get(23)?,
                original_uid: row.get(24)?,
                deleted_at: row.get(25)?,
                archived: row.get(26)?,
                archived_at: row.get(27)?,
            })
        })?;

        let mut contacts = Vec::new();
        for row_result in rows {
            let row = row_result?;
            contacts.push(self.row_to_contact(row)?);
        }

        Ok(contacts)
    }

    /// Lists contacts with pagination support.
    ///
    /// Returns contacts ordered by display_name, starting from `offset`
    /// and returning at most `limit` results.
    pub fn list_contacts_paginated(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Contact>, StorageError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut stmt = self.conn.prepare(
            "SELECT id, public_key, display_name, card_encrypted, shared_key_encrypted,
                    visibility_rules_json, visibility_rules_encrypted, exchange_timestamp,
                    fingerprint_verified, blocked, hidden, favorite, recovery_trusted,
                    proposal_trusted, cek_encrypted, exchange_transport, has_recovered,
                    card_updated_at, relay_url, relay_noise_pubkey, trust_metrics,
                    contact_kind, import_source, imported_at, original_uid,
                    deleted_at, archived, archived_at
             FROM contacts
             WHERE deleted_at IS NULL AND archived = 0
             ORDER BY display_name
             LIMIT ?1 OFFSET ?2",
        )?;

        let rows = stmt.query_map(params![limit as i64, offset as i64], |row| {
            Ok(ContactRow {
                id: row.get(0)?,
                public_key: row.get(1)?,
                display_name: row.get(2)?,
                card_encrypted: row.get(3)?,
                shared_key_encrypted: row.get(4)?,
                visibility_rules_json: row.get(5)?,
                visibility_rules_encrypted: row.get(6)?,
                exchange_timestamp: row.get(7)?,
                fingerprint_verified: row.get(8)?,
                blocked: row.get(9)?,
                hidden: row.get(10)?,
                favorite: row.get(11)?,
                recovery_trusted: row.get(12)?,
                proposal_trusted: row.get(13)?,
                cek_encrypted: row.get(14)?,
                exchange_transport: row.get(15)?,
                has_recovered: row.get(16)?,
                card_updated_at: row.get(17)?,
                relay_url: row.get(18)?,
                relay_noise_pubkey: row.get(19)?,
                trust_metrics: row.get(20)?,
                contact_kind: row.get(21)?,
                import_source: row.get(22)?,
                imported_at: row.get(23)?,
                original_uid: row.get(24)?,
                deleted_at: row.get(25)?,
                archived: row.get(26)?,
                archived_at: row.get(27)?,
            })
        })?;

        let mut contacts = Vec::new();
        for row_result in rows {
            let row = row_result?;
            contacts.push(self.row_to_contact(row)?);
        }

        Ok(contacts)
    }

    /// Searches contacts by display name using case-insensitive matching.
    ///
    /// Returns all contacts whose display_name contains the query string.
    /// An empty query returns all contacts.
    ///
    /// Hybrid approach for performance:
    /// - Legacy contacts (non-empty display_name in DB): searched via SQL LIKE
    /// - CEK-protected contacts (empty display_name in DB): loaded, decrypted,
    ///   and filtered in memory
    pub fn search_contacts(&self, query: &str) -> Result<Vec<Contact>, StorageError> {
        if query.is_empty() {
            return self.list_contacts();
        }

        let pattern = format!("%{}%", query);
        let query_lower = query.to_lowercase();

        // Part 1: SQL search for legacy contacts (non-empty display_name)
        let mut stmt = self.conn.prepare(
            "SELECT id, public_key, display_name, card_encrypted, shared_key_encrypted,
                    visibility_rules_json, visibility_rules_encrypted, exchange_timestamp,
                    fingerprint_verified, blocked, hidden, favorite, recovery_trusted,
                    proposal_trusted, cek_encrypted, exchange_transport, has_recovered,
                    card_updated_at, relay_url, relay_noise_pubkey, trust_metrics,
                    contact_kind, import_source, imported_at, original_uid,
                    deleted_at, archived, archived_at
             FROM contacts
             WHERE display_name != '' AND display_name LIKE ?1 COLLATE NOCASE
               AND deleted_at IS NULL AND archived = 0
             ORDER BY display_name",
        )?;

        let rows = stmt.query_map(params![pattern], |row| {
            Ok(ContactRow {
                id: row.get(0)?,
                public_key: row.get(1)?,
                display_name: row.get(2)?,
                card_encrypted: row.get(3)?,
                shared_key_encrypted: row.get(4)?,
                visibility_rules_json: row.get(5)?,
                visibility_rules_encrypted: row.get(6)?,
                exchange_timestamp: row.get(7)?,
                fingerprint_verified: row.get(8)?,
                blocked: row.get(9)?,
                hidden: row.get(10)?,
                favorite: row.get(11)?,
                recovery_trusted: row.get(12)?,
                proposal_trusted: row.get(13)?,
                cek_encrypted: row.get(14)?,
                exchange_transport: row.get(15)?,
                has_recovered: row.get(16)?,
                card_updated_at: row.get(17)?,
                relay_url: row.get(18)?,
                relay_noise_pubkey: row.get(19)?,
                trust_metrics: row.get(20)?,
                contact_kind: row.get(21)?,
                import_source: row.get(22)?,
                imported_at: row.get(23)?,
                original_uid: row.get(24)?,
                deleted_at: row.get(25)?,
                archived: row.get(26)?,
                archived_at: row.get(27)?,
            })
        })?;

        let mut contacts = Vec::new();
        for row_result in rows {
            let row = row_result?;
            contacts.push(self.row_to_contact(row)?);
        }

        // Part 2: In-memory search for CEK-protected contacts (empty display_name)
        let mut cek_stmt = self.conn.prepare(
            "SELECT id, public_key, display_name, card_encrypted, shared_key_encrypted,
                    visibility_rules_json, visibility_rules_encrypted, exchange_timestamp,
                    fingerprint_verified, blocked, hidden, favorite, recovery_trusted,
                    proposal_trusted, cek_encrypted, exchange_transport, has_recovered,
                    card_updated_at, relay_url, relay_noise_pubkey, trust_metrics,
                    contact_kind, import_source, imported_at, original_uid,
                    deleted_at, archived, archived_at
             FROM contacts
             WHERE display_name = '' AND deleted_at IS NULL AND archived = 0",
        )?;

        let cek_rows = cek_stmt.query_map([], |row| {
            Ok(ContactRow {
                id: row.get(0)?,
                public_key: row.get(1)?,
                display_name: row.get(2)?,
                card_encrypted: row.get(3)?,
                shared_key_encrypted: row.get(4)?,
                visibility_rules_json: row.get(5)?,
                visibility_rules_encrypted: row.get(6)?,
                exchange_timestamp: row.get(7)?,
                fingerprint_verified: row.get(8)?,
                blocked: row.get(9)?,
                hidden: row.get(10)?,
                favorite: row.get(11)?,
                recovery_trusted: row.get(12)?,
                proposal_trusted: row.get(13)?,
                cek_encrypted: row.get(14)?,
                exchange_transport: row.get(15)?,
                has_recovered: row.get(16)?,
                card_updated_at: row.get(17)?,
                relay_url: row.get(18)?,
                relay_noise_pubkey: row.get(19)?,
                trust_metrics: row.get(20)?,
                contact_kind: row.get(21)?,
                import_source: row.get(22)?,
                imported_at: row.get(23)?,
                original_uid: row.get(24)?,
                deleted_at: row.get(25)?,
                archived: row.get(26)?,
                archived_at: row.get(27)?,
            })
        })?;

        for row_result in cek_rows {
            let row = row_result?;
            let contact = self.row_to_contact(row)?;
            if contact.display_name().to_lowercase().contains(&query_lower) {
                contacts.push(contact);
            }
        }

        // Sort combined results by display_name
        contacts.sort_by(|a, b| {
            a.display_name()
                .to_lowercase()
                .cmp(&b.display_name().to_lowercase())
        });

        Ok(contacts)
    }

    /// Deletes a contact by ID.
    pub fn delete_contact(&self, id: &str) -> Result<bool, StorageError> {
        // Also delete associated ratchet state
        self.conn.execute(
            "DELETE FROM contact_ratchets WHERE contact_id = ?1",
            params![id],
        )?;

        let rows_affected = self
            .conn
            .execute("DELETE FROM contacts WHERE id = ?1", params![id])?;
        Ok(rows_affected > 0)
    }

    // === Archived Contacts ===

    /// Lists contacts that are archived (but not soft-deleted).
    pub fn list_archived_contacts(&self) -> Result<Vec<Contact>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, public_key, display_name, card_encrypted, shared_key_encrypted,
                    visibility_rules_json, visibility_rules_encrypted, exchange_timestamp,
                    fingerprint_verified, blocked, hidden, favorite, recovery_trusted,
                    proposal_trusted, cek_encrypted, exchange_transport, has_recovered,
                    card_updated_at, relay_url, relay_noise_pubkey, trust_metrics,
                    contact_kind, import_source, imported_at, original_uid,
                    deleted_at, archived, archived_at
             FROM contacts
             WHERE archived = 1 AND deleted_at IS NULL
             ORDER BY display_name",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(ContactRow {
                id: row.get(0)?,
                public_key: row.get(1)?,
                display_name: row.get(2)?,
                card_encrypted: row.get(3)?,
                shared_key_encrypted: row.get(4)?,
                visibility_rules_json: row.get(5)?,
                visibility_rules_encrypted: row.get(6)?,
                exchange_timestamp: row.get(7)?,
                fingerprint_verified: row.get(8)?,
                blocked: row.get(9)?,
                hidden: row.get(10)?,
                favorite: row.get(11)?,
                recovery_trusted: row.get(12)?,
                proposal_trusted: row.get(13)?,
                cek_encrypted: row.get(14)?,
                exchange_transport: row.get(15)?,
                has_recovered: row.get(16)?,
                card_updated_at: row.get(17)?,
                relay_url: row.get(18)?,
                relay_noise_pubkey: row.get(19)?,
                trust_metrics: row.get(20)?,
                contact_kind: row.get(21)?,
                import_source: row.get(22)?,
                imported_at: row.get(23)?,
                original_uid: row.get(24)?,
                deleted_at: row.get(25)?,
                archived: row.get(26)?,
                archived_at: row.get(27)?,
            })
        })?;

        let mut contacts = Vec::new();
        for row_result in rows {
            let row = row_result?;
            contacts.push(self.row_to_contact(row)?);
        }

        Ok(contacts)
    }

    // === Stale Soft-Delete Cleanup ===

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

    // === Import Dedup ===

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
}
