// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The user's own card, one encrypted row in a table.
//!
//! A handle rather than two methods on `ContactStore` so a caller can be
//! handed the slot it may read and write without knowing which table that
//! is: the real card for the engine and a normal session, a duress copy for
//! what a duress session shows and edits (ADR-032, vauchi/private#811).

use rusqlite::{Connection, params};
use std::sync::Arc;

use super::super::StorageError;
use super::contact::ContactStore;
use crate::card::ContactCard;
use crate::clock::Clock;
use crate::crypto::SymmetricKey;

/// One own-card row: `table` has `id = 1`, `card_json`,
/// `card_json_encrypted` and `updated_at`.
pub struct OwnCardStore<'a> {
    conn: &'a Connection,
    key: &'a SymmetricKey,
    clock: &'a Arc<dyn Clock>,
    table: &'static str,
}

impl<'a> ContactStore<'a> {
    /// The real own card.
    pub fn own_card(&self) -> OwnCardStore<'a> {
        self.own_card_in("own_card")
    }

    /// The copy a duress session shows and edits (vauchi/private#811).
    pub fn duress_own_card(&self) -> OwnCardStore<'a> {
        self.own_card_in("duress_own_card")
    }

    fn own_card_in(&self, table: &'static str) -> OwnCardStore<'a> {
        OwnCardStore {
            conn: self.conn,
            key: self.key,
            clock: self.clock,
            table,
        }
    }
}

impl OwnCardStore<'_> {
    /// Saves the card (encrypted).
    pub fn save(&self, card: &ContactCard) -> Result<(), StorageError> {
        let card_json =
            serde_json::to_string(card).map_err(|e| StorageError::Serialization(e.to_string()))?;

        let card_encrypted = crate::crypto::encrypt(self.key, card_json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.clock.unix_seconds();

        self.conn.execute(
            &format!(
                "INSERT OR REPLACE INTO {} (id, card_json, card_json_encrypted, updated_at) VALUES (1, '', ?1, ?2)",
                self.table
            ),
            params![card_encrypted, now as i64],
        )?;

        Ok(())
    }

    /// Loads the card (decrypted).
    ///
    /// Reads from the encrypted column first; falls back to plaintext for
    /// pre-v13 databases where migration has not populated it.
    pub fn load(&self) -> Result<Option<ContactCard>, StorageError> {
        let result = self.conn.query_row(
            &format!(
                "SELECT card_json_encrypted, card_json FROM {} WHERE id = 1",
                self.table
            ),
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: String = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let mut card: ContactCard = serde_json::from_str(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                card.resolve_stored_network_ids();
                Ok(Some(card))
            }
            Ok((_, plaintext)) if !plaintext.is_empty() => {
                let mut card: ContactCard = serde_json::from_str(&plaintext)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                card.resolve_stored_network_ids();
                Ok(Some(card))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
}
