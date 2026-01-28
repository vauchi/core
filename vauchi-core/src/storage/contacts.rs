// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact storage operations.

use rusqlite::params;

use super::{Storage, StorageError};
use crate::contact::Contact;
use crate::contact_card::ContactCard;
use crate::crypto::SymmetricKey;

/// Internal struct for database row data.
#[allow(dead_code)] // Fields are used via destructuring in row_to_contact
pub(super) struct ContactRow {
    pub id: String,
    pub public_key: Vec<u8>,
    pub display_name: String,
    pub card_encrypted: Vec<u8>,
    pub shared_key_encrypted: Vec<u8>,
    pub visibility_rules_json: Option<String>,
    pub exchange_timestamp: i64,
    pub fingerprint_verified: i32,
}

impl Storage {
    // === Contact Operations ===

    /// Saves a contact to storage.
    pub fn save_contact(&self, contact: &Contact) -> Result<(), StorageError> {
        // Serialize and encrypt the contact card
        let card_json = serde_json::to_vec(contact.card())
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let card_encrypted = crate::crypto::encrypt(&self.encryption_key, &card_json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        // Encrypt the shared key
        let shared_key_encrypted =
            crate::crypto::encrypt(&self.encryption_key, contact.shared_key().as_bytes())
                .map_err(|e| StorageError::Encryption(e.to_string()))?;

        // Serialize visibility rules
        let visibility_json = serde_json::to_string(contact.visibility_rules())
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        self.conn.execute(
            "INSERT OR REPLACE INTO contacts
             (id, public_key, display_name, card_encrypted, shared_key_encrypted,
              visibility_rules_json, exchange_timestamp, fingerprint_verified, last_sync_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                contact.id(),
                contact.public_key().as_slice(),
                contact.display_name(),
                card_encrypted,
                shared_key_encrypted,
                visibility_json,
                contact.exchange_timestamp() as i64,
                contact.is_fingerprint_verified() as i32,
                Option::<i64>::None,
            ],
        )?;

        Ok(())
    }

    /// Loads a contact by ID.
    pub fn load_contact(&self, id: &str) -> Result<Option<Contact>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, public_key, display_name, card_encrypted, shared_key_encrypted,
                    visibility_rules_json, exchange_timestamp, fingerprint_verified
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
                exchange_timestamp: row.get(6)?,
                fingerprint_verified: row.get(7)?,
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
                    visibility_rules_json, exchange_timestamp, fingerprint_verified
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
                exchange_timestamp: row.get(6)?,
                fingerprint_verified: row.get(7)?,
            })
        })?;

        let mut contacts = Vec::new();
        for row_result in rows {
            let row = row_result?;
            contacts.push(self.row_to_contact(row)?);
        }

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

    /// Converts a database row to a Contact.
    pub(super) fn row_to_contact(&self, row: ContactRow) -> Result<Contact, StorageError> {
        // Decrypt card
        let card_json = crate::crypto::decrypt(&self.encryption_key, &row.card_encrypted)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let card: ContactCard = serde_json::from_slice(&card_json)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        // Decrypt shared key
        let shared_key_bytes =
            crate::crypto::decrypt(&self.encryption_key, &row.shared_key_encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let shared_key_array: [u8; 32] = shared_key_bytes
            .try_into()
            .map_err(|_| StorageError::Encryption("Invalid key length".into()))?;
        let shared_key = SymmetricKey::from_bytes(shared_key_array);

        // Parse public key
        let public_key: [u8; 32] = row
            .public_key
            .try_into()
            .map_err(|_| StorageError::Encryption("Invalid public key length".into()))?;

        // Create contact
        let mut contact = Contact::from_exchange(public_key, card, shared_key);

        // Parse and apply visibility rules
        if let Some(json) = row.visibility_rules_json {
            let rules = serde_json::from_str(&json)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            *contact.visibility_rules_mut() = rules;
        }

        // Set fingerprint verification
        if row.fingerprint_verified != 0 {
            contact.mark_fingerprint_verified();
        }

        Ok(contact)
    }

    // === Own Contact Card Operations ===

    /// Saves the user's own contact card.
    pub fn save_own_card(&self, card: &ContactCard) -> Result<(), StorageError> {
        let card_json =
            serde_json::to_string(card).map_err(|e| StorageError::Serialization(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO own_card (id, card_json, updated_at) VALUES (1, ?1, ?2)",
            params![card_json, now as i64],
        )?;

        Ok(())
    }

    /// Loads the user's own contact card.
    pub fn load_own_card(&self) -> Result<Option<ContactCard>, StorageError> {
        let result =
            self.conn
                .query_row("SELECT card_json FROM own_card WHERE id = 1", [], |row| {
                    row.get::<_, String>(0)
                });

        match result {
            Ok(json) => {
                let card = serde_json::from_str(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(card))
            }
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
        self.conn.execute(
            "INSERT OR REPLACE INTO contact_sync_timestamps (contact_id, last_sync_at)
             VALUES (?1, ?2)",
            params![contact_id, timestamp as i64],
        )?;
        Ok(())
    }

    /// Gets the last sync timestamp for a contact.
    ///
    /// Returns None if the contact hasn't been synced yet.
    pub fn get_contact_last_sync(&self, contact_id: &str) -> Result<Option<u64>, StorageError> {
        let result = self.conn.query_row(
            "SELECT last_sync_at FROM contact_sync_timestamps WHERE contact_id = ?1",
            params![contact_id],
            |row| row.get::<_, i64>(0),
        );

        match result {
            Ok(timestamp) => Ok(Some(timestamp as u64)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
}
