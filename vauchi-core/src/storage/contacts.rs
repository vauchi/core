// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact storage operations.

use rusqlite::params;

use super::{Storage, StorageError};
use crate::contact::Contact;
use crate::contact_card::ContactCard;
use crate::crypto::SymmetricKey;
use crate::crypto::cek::ContentEncryptionKey;
use crate::exchange::{ExchangeTransport, TrustMetrics};

/// Internal struct for database row data.
#[allow(dead_code)] // Fields are used via destructuring in row_to_contact
pub(super) struct ContactRow {
    pub id: String,
    pub public_key: Vec<u8>,
    pub display_name: String,
    pub card_encrypted: Vec<u8>,
    pub shared_key_encrypted: Vec<u8>,
    pub visibility_rules_json: Option<String>,
    pub visibility_rules_encrypted: Option<Vec<u8>>,
    pub exchange_timestamp: i64,
    pub fingerprint_verified: i32,
    pub blocked: i32,
    pub hidden: i32,
    pub favorite: i32,
    pub recovery_trusted: i32,
    pub proposal_trusted: i32,
    pub cek_encrypted: Option<Vec<u8>>,
    pub exchange_transport: String,
    pub has_recovered: i32,
    pub card_updated_at: Option<i64>,
    pub relay_url: Option<String>,
    pub relay_noise_pubkey: Option<Vec<u8>>,
    pub trust_metrics: Option<String>,
}

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
                // CEK path: encrypt card with CEK, empty display_name (no plaintext
                // personal data in DB), persist CEK encrypted with storage key
                let card_ct = cek
                    .encrypt(&card_json)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let cek_ct = crate::crypto::encrypt(&self.encryption_key, &cek.to_bytes())
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                // Empty string instead of NULL because column is NOT NULL.
                // No personal data leaks — the display name is only inside the
                // CEK-encrypted card content.
                (card_ct, String::new(), Some(cek_ct))
            } else {
                // Legacy path: encrypt card with storage key, plaintext display_name
                let card_ct = crate::crypto::encrypt(&self.encryption_key, &card_json)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                (card_ct, contact.display_name().to_string(), None::<Vec<u8>>)
            };

        // Encrypt the shared key
        let shared_key_encrypted =
            crate::crypto::encrypt(&self.encryption_key, contact.shared_key().as_bytes())
                .map_err(|e| StorageError::Encryption(e.to_string()))?;

        // Encrypt visibility rules
        let visibility_json = serde_json::to_string(contact.visibility_rules())
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let visibility_encrypted =
            crate::crypto::encrypt(&self.encryption_key, visibility_json.as_bytes())
                .map_err(|e| StorageError::Encryption(e.to_string()))?;

        // Serialize exchange_transport as its serde name ("qr"/"nfc"/"ble"/"usb"/"audio")
        let transport_str = serde_json::to_value(contact.exchange_transport())
            .map_err(|e| StorageError::Serialization(e.to_string()))?
            .as_str()
            .unwrap_or("Qr")
            .to_string();

        // Serialize trust_metrics as JSON (None → NULL)
        let trust_metrics_json = contact
            .trust_metrics()
            .map(|m| serde_json::to_string(m).expect("trust_metrics serialize"));

        // Use INSERT ... ON CONFLICT DO UPDATE (upsert) rather than INSERT OR REPLACE.
        //
        // INSERT OR REPLACE deletes the old row before inserting the new one, which
        // triggers the ON DELETE CASCADE on `contact_field_notes` and silently wipes
        // all per-field private notes every time a contact is updated. The upsert form
        // updates the row in-place, preserving child rows.
        self.conn.execute(
            "INSERT INTO contacts
             (id, public_key, display_name, card_encrypted, shared_key_encrypted,
              visibility_rules_encrypted, exchange_timestamp, fingerprint_verified, last_sync_at,
              blocked, hidden, favorite, recovery_trusted, proposal_trusted, cek_encrypted,
              exchange_transport, has_recovered, card_updated_at,
              relay_url, relay_noise_pubkey, trust_metrics)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
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
               trust_metrics           = excluded.trust_metrics",
            params![
                contact.id(),
                contact.public_key().as_slice(),
                display_name_db,
                card_encrypted,
                shared_key_encrypted,
                visibility_encrypted,
                contact.exchange_timestamp() as i64,
                contact.is_fingerprint_verified() as i32,
                Option::<i64>::None,
                contact.is_blocked() as i32,
                contact.is_hidden() as i32,
                contact.is_favorite() as i32,
                contact.is_recovery_trusted() as i32,
                contact.is_proposal_trusted() as i32,
                cek_encrypted_param,
                transport_str,
                contact.has_recovered() as i32,
                contact.card_updated_at().map(|t| t as i64),
                contact.relay_url(),
                contact.relay_noise_pubkey().map(|k| k.to_vec()),
                trust_metrics_json,
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
                    card_updated_at, relay_url, relay_noise_pubkey, trust_metrics
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
            })
        });

        match result {
            Ok(row) => Ok(Some(self.row_to_contact(row)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Lists all contacts.
    pub fn list_contacts(&self) -> Result<Vec<Contact>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, public_key, display_name, card_encrypted, shared_key_encrypted,
                    visibility_rules_json, visibility_rules_encrypted, exchange_timestamp,
                    fingerprint_verified, blocked, hidden, favorite, recovery_trusted,
                    proposal_trusted, cek_encrypted, exchange_transport, has_recovered,
                    card_updated_at, relay_url, relay_noise_pubkey, trust_metrics
             FROM contacts ORDER BY display_name",
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
                    card_updated_at, relay_url, relay_noise_pubkey, trust_metrics
             FROM contacts ORDER BY display_name
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
                    card_updated_at, relay_url, relay_noise_pubkey, trust_metrics
             FROM contacts
             WHERE display_name != '' AND display_name LIKE ?1 COLLATE NOCASE
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
                    card_updated_at, relay_url, relay_noise_pubkey, trust_metrics
             FROM contacts
             WHERE display_name = ''",
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

    // === Personal Notes Operations ===

    /// Saves encrypted personal notes for a contact.
    ///
    /// Updates the `personal_notes_encrypted` column for the given contact.
    /// The caller is responsible for encrypting the notes before passing them in.
    pub fn save_personal_notes(
        &self,
        contact_id: &str,
        notes_encrypted: &[u8],
    ) -> Result<(), StorageError> {
        let rows_affected = self.conn.execute(
            "UPDATE contacts SET personal_notes_encrypted = ?1 WHERE id = ?2",
            params![notes_encrypted, contact_id],
        )?;

        if rows_affected == 0 {
            return Err(StorageError::NotFound(format!(
                "Contact not found: {}",
                contact_id
            )));
        }

        Ok(())
    }

    /// Loads encrypted personal notes for a contact.
    ///
    /// Returns `None` if the contact has no personal notes stored.
    pub fn load_personal_notes(&self, contact_id: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let result = self.conn.query_row(
            "SELECT personal_notes_encrypted FROM contacts WHERE id = ?1",
            params![contact_id],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );

        match result {
            Ok(notes) => Ok(notes),
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(StorageError::NotFound(format!(
                "Contact not found: {}",
                contact_id
            ))),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Deletes personal notes for a contact.
    ///
    /// Sets the `personal_notes_encrypted` column to NULL.
    pub fn delete_personal_notes(&self, contact_id: &str) -> Result<(), StorageError> {
        let rows_affected = self.conn.execute(
            "UPDATE contacts SET personal_notes_encrypted = NULL WHERE id = ?1",
            params![contact_id],
        )?;

        if rows_affected == 0 {
            return Err(StorageError::NotFound(format!(
                "Contact not found: {}",
                contact_id
            )));
        }

        Ok(())
    }

    // Contact field notes: see storage/field_notes.rs

    // === Avatar Operations ===

    /// Saves an encrypted avatar for a contact.
    ///
    /// Updates the `avatar_encrypted` column for the given contact.
    /// The caller is responsible for encrypting the avatar before passing it in.
    pub fn save_avatar(
        &self,
        contact_id: &str,
        avatar_encrypted: &[u8],
    ) -> Result<(), StorageError> {
        let rows_affected = self.conn.execute(
            "UPDATE contacts SET avatar_encrypted = ?1 WHERE id = ?2",
            params![avatar_encrypted, contact_id],
        )?;

        if rows_affected == 0 {
            return Err(StorageError::NotFound(format!(
                "Contact not found: {}",
                contact_id
            )));
        }

        Ok(())
    }

    /// Loads an encrypted avatar for a contact.
    ///
    /// Returns `None` if the contact has no avatar stored.
    pub fn load_avatar(&self, contact_id: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let result = self.conn.query_row(
            "SELECT avatar_encrypted FROM contacts WHERE id = ?1",
            params![contact_id],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );

        match result {
            Ok(avatar) => Ok(avatar),
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(StorageError::NotFound(format!(
                "Contact not found: {}",
                contact_id
            ))),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    // === Contact Count & Limits ===

    /// Counts the total number of contacts in storage.
    pub fn count_contacts(&self) -> Result<usize, StorageError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM contacts", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Returns the maximum number of contacts allowed.
    ///
    /// Reads from the `contact_limits` table (created by migration v4).
    /// Returns 10,000 as the default if no limit has been configured.
    pub fn get_contact_limit(&self) -> Result<usize, StorageError> {
        let result = self.conn.query_row(
            "SELECT max_contacts FROM contact_limits WHERE id = 1",
            [],
            |row| row.get::<_, i64>(0),
        );

        match result {
            Ok(limit) => Ok(limit as usize),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(10_000), // Default limit
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Sets the maximum number of contacts allowed.
    ///
    /// Updates the `contact_limits` table (created by migration v4).
    pub fn set_contact_limit(&self, max_contacts: usize) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO contact_limits (id, max_contacts) VALUES (1, ?1)",
            params![max_contacts as i64],
        )?;
        Ok(())
    }

    /// Converts a database row to a Contact.
    ///
    /// CEK-aware: if `cek_encrypted` is present, decrypts the CEK with the
    /// storage key, then decrypts the card with the CEK. Otherwise, decrypts
    /// the card with the storage key (legacy path).
    pub(super) fn row_to_contact(&self, row: ContactRow) -> Result<Contact, StorageError> {
        // Decrypt card — CEK path or legacy path
        let (card, cek) = if let Some(ref cek_encrypted) = row.cek_encrypted {
            // CEK path: decrypt CEK with storage key, then card with CEK
            let cek_bytes = crate::crypto::decrypt(&self.encryption_key, cek_encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let cek_array: [u8; 32] = cek_bytes
                .try_into()
                .map_err(|_| StorageError::Encryption("Invalid CEK length".into()))?;
            let cek = ContentEncryptionKey::from_bytes(cek_array);

            let card_json = cek
                .decrypt(&row.card_encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let card: ContactCard = serde_json::from_slice(&card_json)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            (card, Some(cek))
        } else {
            // Legacy path: decrypt card with storage key
            let card_json = crate::crypto::decrypt(&self.encryption_key, &row.card_encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let card: ContactCard = serde_json::from_slice(&card_json)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            (card, None)
        };

        // Decrypt shared key
        let shared_key_bytes =
            crate::crypto::decrypt(&self.encryption_key, &row.shared_key_encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let shared_key_array: [u8; 32] = shared_key_bytes
            .try_into()
            .map_err(|_| StorageError::Encryption("Invalid key length".into()))?;
        let shared_key = SymmetricKey::from_bytes_unchecked(shared_key_array);

        // Parse public key
        let public_key: [u8; 32] = row
            .public_key
            .try_into()
            .map_err(|_| StorageError::Encryption("Invalid public key length".into()))?;

        // Parse visibility rules — prefer encrypted column, fall back to legacy plaintext
        let visibility_rules = if let Some(encrypted) = row.visibility_rules_encrypted {
            let json_bytes = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let json = String::from_utf8(json_bytes)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            serde_json::from_str(&json).map_err(|e| StorageError::Serialization(e.to_string()))?
        } else if let Some(json) = row.visibility_rules_json {
            serde_json::from_str(&json).map_err(|e| StorageError::Serialization(e.to_string()))?
        } else {
            crate::contact::VisibilityRules::new()
        };

        // Create contact with all persisted fields
        let mut contact = Contact::from_sync_data_full(
            public_key,
            card,
            shared_key,
            row.exchange_timestamp as u64,
            row.fingerprint_verified != 0,
            visibility_rules,
            row.hidden != 0,
            row.blocked != 0,
            row.recovery_trusted != 0,
        );

        // Restore favorite flag from storage
        if row.favorite != 0 {
            contact.set_favorite(true);
        }

        // Restore proposal_trusted flag from storage
        if row.proposal_trusted != 0 {
            contact.set_proposal_trusted(true);
        }

        // Attach CEK if this contact is CEK-protected
        if let Some(cek) = cek {
            contact.set_cek(cek);
        }

        // Restore trust metric fields from storage
        let transport: ExchangeTransport =
            serde_json::from_value(serde_json::Value::String(row.exchange_transport))
                .unwrap_or_default();
        contact.set_exchange_transport(transport);
        contact.set_has_recovered(row.has_recovered != 0);
        contact.set_card_updated_at(row.card_updated_at.map(|t| t as u64));

        // Restore relay fields from storage
        contact.set_relay_url(row.relay_url);
        if let Some(pubkey_bytes) = row.relay_noise_pubkey {
            let pubkey: [u8; 32] = pubkey_bytes.try_into().map_err(|_| {
                StorageError::Encryption("Invalid relay Noise pubkey length".into())
            })?;
            contact.set_relay_noise_pubkey(Some(pubkey));
        }

        // Restore trust metrics from storage (JSON column, NULL for legacy contacts)
        let trust_metrics: Option<TrustMetrics> = row
            .trust_metrics
            .map(|s| serde_json::from_str(&s).expect("trust_metrics deserialize"));
        contact.set_trust_metrics(trust_metrics);

        Ok(contact)
    }

    // === Own Contact Card Operations ===

    /// Saves the user's own contact card (encrypted).
    pub fn save_own_card(&self, card: &ContactCard) -> Result<(), StorageError> {
        let card_json =
            serde_json::to_string(card).map_err(|e| StorageError::Serialization(e.to_string()))?;

        let card_encrypted = crate::crypto::encrypt(&self.encryption_key, card_json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO own_card (id, card_json, card_json_encrypted, updated_at) VALUES (1, '', ?1, ?2)",
            params![card_encrypted, now as i64],
        )?;

        Ok(())
    }

    /// Loads the user's own contact card (decrypted).
    ///
    /// Reads from encrypted column first; falls back to plaintext for
    /// pre-v13 databases where migration hasn't populated the encrypted column.
    pub fn load_own_card(&self) -> Result<Option<ContactCard>, StorageError> {
        let result = self.conn.query_row(
            "SELECT card_json_encrypted, card_json FROM own_card WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: String = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let card = serde_json::from_str(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(card))
            }
            Ok((_, plaintext)) if !plaintext.is_empty() => {
                // Fallback: read from plaintext column (pre-v13 data)
                let card = serde_json::from_str(&plaintext)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(card))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    // === Sync Timestamp Operations ===

    /// Sets the last sync timestamp for a contact.
    ///
    /// This is used to track when the last successful sync occurred.
    /// Uses a separate table from contacts to allow tracking sync timestamps
    /// independently of whether the contact exists in the contacts table.
    pub fn set_contact_last_sync(
        &self,
        contact_id: &str,
        timestamp: u64,
    ) -> Result<(), StorageError> {
        let ts_bytes = (timestamp as i64).to_le_bytes();
        let encrypted = crate::crypto::encrypt(&self.encryption_key, &ts_bytes)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        self.conn.execute(
            "INSERT OR REPLACE INTO contact_sync_timestamps (contact_id, last_sync_at, last_sync_at_encrypted)
             VALUES (?1, ?2, ?3)",
            params![contact_id, timestamp as i64, encrypted],
        )?;
        Ok(())
    }

    /// Gets the last sync timestamp for a contact (decrypted).
    ///
    /// Returns None if the contact hasn't been synced yet.
    pub fn get_contact_last_sync(&self, contact_id: &str) -> Result<Option<u64>, StorageError> {
        let result = self.conn.query_row(
            "SELECT last_sync_at_encrypted, last_sync_at FROM contact_sync_timestamps WHERE contact_id = ?1",
            params![contact_id],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: i64 = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                if decrypted.len() == 8 {
                    let ts = i64::from_le_bytes(
                        decrypted
                            .try_into()
                            .map_err(|_| StorageError::Encryption("Invalid timestamp".into()))?,
                    );
                    Ok(Some(ts as u64))
                } else {
                    Err(StorageError::Encryption("Invalid timestamp length".into()))
                }
            }
            Ok((_, plaintext)) => Ok(Some(plaintext as u64)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    // === Content Encryption Key (CEK) Operations ===

    /// Saves a CEK for a contact, encrypted with the storage master key.
    ///
    /// The CEK controls at-rest readability of the contact card (crypto-shredding).
    pub fn save_contact_cek(
        &self,
        contact_id: &str,
        cek: &ContentEncryptionKey,
    ) -> Result<(), StorageError> {
        let cek_encrypted = crate::crypto::encrypt(&self.encryption_key, &cek.to_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let rows_affected = self.conn.execute(
            "UPDATE contacts SET cek_encrypted = ?1 WHERE id = ?2",
            params![cek_encrypted, contact_id],
        )?;

        if rows_affected == 0 {
            return Err(StorageError::NotFound(format!(
                "Contact not found: {}",
                contact_id
            )));
        }

        Ok(())
    }

    /// Loads the CEK for a contact. Returns None for legacy contacts (pre-CEK).
    pub fn load_contact_cek(
        &self,
        contact_id: &str,
    ) -> Result<Option<ContentEncryptionKey>, StorageError> {
        let result = self.conn.query_row(
            "SELECT cek_encrypted FROM contacts WHERE id = ?1",
            params![contact_id],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );

        match result {
            Ok(Some(cek_encrypted)) => {
                let cek_bytes = crate::crypto::decrypt(&self.encryption_key, &cek_encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let cek_array: [u8; 32] = cek_bytes
                    .try_into()
                    .map_err(|_| StorageError::Encryption("Invalid CEK length".into()))?;
                Ok(Some(ContentEncryptionKey::from_bytes(cek_array)))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(StorageError::NotFound(format!(
                "Contact not found: {}",
                contact_id
            ))),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Deletes the CEK for a contact (crypto-shredding).
    ///
    /// Sets `cek_encrypted` to NULL, rendering the card permanently unreadable
    /// if it was encrypted with the CEK.
    pub fn delete_contact_cek(&self, contact_id: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE contacts SET cek_encrypted = NULL WHERE id = ?1",
            params![contact_id],
        )?;
        Ok(())
    }

    /// Returns the last applied delta version for a contact (#42).
    ///
    /// Returns 0 if no version has been recorded (new or legacy contact).
    pub fn last_delta_version(&self, contact_id: &str) -> Result<u32, StorageError> {
        let version: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(last_delta_version, 0) FROM contacts WHERE id = ?1",
                params![contact_id],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NotFound(format!("Contact: {}", contact_id))
                }
                other => StorageError::Database(other),
            })?;
        Ok(version as u32)
    }

    /// Records the last applied delta version for a contact (#42).
    pub fn record_delta_version(&self, contact_id: &str, version: u32) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE contacts SET last_delta_version = ?1 WHERE id = ?2",
            params![version as i64, contact_id],
        )?;
        Ok(())
    }

    // === Revoked Senders Operations ===

    /// Records a revoked sender in the tombstone table.
    ///
    /// Prevents future updates from this sender from being processed,
    /// even if the contact row has been deleted.
    pub fn record_revoked_sender(
        &self,
        sender_id: &str,
        revoked_at: u64,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO revoked_senders (sender_id, revoked_at) VALUES (?1, ?2)",
            params![sender_id, revoked_at as i64],
        )?;
        Ok(())
    }

    /// Checks if a sender has been revoked.
    pub fn is_sender_revoked(&self, sender_id: &str) -> Result<bool, StorageError> {
        let result = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM revoked_senders WHERE sender_id = ?1",
            params![sender_id],
            |row| row.get::<_, bool>(0),
        );

        match result {
            Ok(revoked) => Ok(revoked),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    // === Dismissed Duplicates Operations ===

    /// Records a dismissed duplicate pair.
    ///
    /// The pair is normalized so id1 < id2 lexicographically, ensuring
    /// (A, B) and (B, A) are stored identically.
    pub fn dismiss_duplicate(&self, id1: &str, id2: &str) -> Result<(), StorageError> {
        let (norm_id1, norm_id2) = crate::contact::merge::normalize_pair_key(id1, id2);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();
        self.conn.execute(
            "INSERT OR IGNORE INTO dismissed_duplicates (id1, id2, dismissed_at) VALUES (?1, ?2, ?3)",
            params![norm_id1, norm_id2, now as i64],
        )?;
        Ok(())
    }

    /// Loads all dismissed duplicate pairs.
    ///
    /// Returns a set of (id1, id2) tuples where id1 < id2 lexicographically.
    pub fn load_dismissed_duplicates(
        &self,
    ) -> Result<std::collections::HashSet<(String, String)>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id1, id2 FROM dismissed_duplicates")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut dismissed = std::collections::HashSet::new();
        for row_result in rows {
            dismissed.insert(row_result?);
        }
        Ok(dismissed)
    }

    /// Removes a dismissed duplicate pair (e.g., when contacts are deleted).
    pub fn undismiss_duplicate(&self, id1: &str, id2: &str) -> Result<(), StorageError> {
        let (norm_id1, norm_id2) = crate::contact::merge::normalize_pair_key(id1, id2);
        self.conn.execute(
            "DELETE FROM dismissed_duplicates WHERE id1 = ?1 AND id2 = ?2",
            params![norm_id1, norm_id2],
        )?;
        Ok(())
    }
}
