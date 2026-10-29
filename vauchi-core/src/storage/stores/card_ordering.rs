// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Durable peer-card ordering, separate from cached owner-device stamps.

use rusqlite::{OptionalExtension, params};

use super::ContactStore;
use crate::crypto::{decrypt, encrypt};
use crate::storage::StorageError;

impl ContactStore<'_> {
    pub(crate) fn load_card_ordering(
        &self,
        contact_id: &str,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        let encrypted: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT state_encrypted FROM peer_card_ordering WHERE contact_id = ?1",
                [contact_id],
                |row| row.get(0),
            )
            .optional()?;
        encrypted
            .map(|bytes| {
                decrypt(self.key, &bytes).map_err(|e| StorageError::Encryption(e.to_string()))
            })
            .transpose()
    }

    pub(crate) fn save_card_ordering(
        &self,
        contact_id: &str,
        state: &[u8],
    ) -> Result<(), StorageError> {
        let encrypted =
            encrypt(self.key, state).map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO peer_card_ordering (contact_id, state_encrypted) VALUES (?1, ?2)
             ON CONFLICT(contact_id) DO UPDATE SET state_encrypted = excluded.state_encrypted",
            params![contact_id, encrypted],
        )?;
        Ok(())
    }

    #[cfg(feature = "testing")]
    pub fn test_load_card_ordering(
        &self,
        contact_id: &str,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        self.load_card_ordering(contact_id)
    }

    #[cfg(feature = "testing")]
    pub fn test_save_card_ordering(
        &self,
        contact_id: &str,
        state: &[u8],
    ) -> Result<(), StorageError> {
        self.save_card_ordering(contact_id, state)
    }
}
