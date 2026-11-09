// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The inbox of updates a locked run received (ADR-033 addendum 2026-11-09,
//! point 1, vauchi/private#1093 slice 6). Each item is sealed to the inbox
//! public key with a fresh ephemeral key; the private key is under the
//! storage key. A locked phone can add items but not read them, so a copy of
//! it taken later does not hold the updates already received.

use rusqlite::{Connection, OptionalExtension, params};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use crate::crypto::{SymmetricKey, X3DHKeyPair, decrypt, encrypt, kdf::HKDF};
use crate::recovery::sealed_box;

use super::super::{Storage, StorageError};

const INBOX_SEAL_INFO: &[u8] = b"Vauchi_Receive_Inbox_Seal_v1";

fn derive_inbox_seal_key(ikm: &[u8]) -> Zeroizing<[u8; 32]> {
    HKDF::derive_key(None, ikm, INBOX_SEAL_INFO)
}

/// One opened inbox item.
pub struct InboxItem {
    /// Removes the item once it has been merged.
    pub id: i64,
    /// What the locked run sealed.
    pub update: Zeroizing<Vec<u8>>,
}

/// The inbox, oldest first. An item that does not open is listed by id so
/// the caller can drop it; it never blocks the others.
pub struct InboxItems {
    pub readable: Vec<InboxItem>,
    pub unreadable: Vec<i64>,
}

/// Scoped persistence view for the receive inbox.
pub struct ReceiveInboxStore<'a> {
    conn: &'a Connection,
    storage_key: Option<&'a SymmetricKey>,
}

impl<'a> ReceiveInboxStore<'a> {
    /// The view a locked run has: it can put items and read none.
    pub(in crate::storage) fn write_only(conn: &'a Connection) -> Self {
        Self {
            conn,
            storage_key: None,
        }
    }
}

impl Storage {
    /// Scoped persistence view for the receive inbox.
    pub fn receive_inbox(&self) -> crate::storage::Sealed<'_, ReceiveInboxStore<'_>> {
        self.sealed(ReceiveInboxStore {
            conn: &self.conn,
            storage_key: Some(&self.encryption_key),
        })
    }
}

/// Makes the inbox keypair if the file has none; an existing one is kept,
/// so items sealed to it stay readable.
pub(in crate::storage) fn ensure_inbox_key(
    conn: &Connection,
    storage_key: &SymmetricKey,
) -> Result<(), StorageError> {
    let exists: Option<i64> = conn
        .query_row("SELECT id FROM inbox_key WHERE id = 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    if exists.is_some() {
        return Ok(());
    }
    let keypair = X3DHKeyPair::generate();
    let private_key_encrypted = encrypt(storage_key, keypair.secret_bytes().as_ref())
        .map_err(|e| StorageError::Encryption(e.to_string()))?;
    conn.execute(
        "INSERT INTO inbox_key (id, public_key, private_key_encrypted) VALUES (1, ?1, ?2)",
        params![keypair.public_key().as_slice(), private_key_encrypted],
    )?;
    Ok(())
}

impl ReceiveInboxStore<'_> {
    fn public_key(&self) -> Result<PublicKey, StorageError> {
        let bytes: Option<Vec<u8>> = self
            .conn
            .query_row("SELECT public_key FROM inbox_key WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()?;
        let bytes: [u8; 32] = bytes
            .ok_or(StorageError::ReceiveKeyUnavailable)?
            .try_into()
            .map_err(|_| StorageError::Encryption("inbox public key length".into()))?;
        Ok(PublicKey::from(bytes))
    }

    fn private_key(&self) -> Result<Option<StaticSecret>, StorageError> {
        let storage_key = self
            .storage_key
            .ok_or(StorageError::StorageKeyUnavailable)?;
        let sealed: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT private_key_encrypted FROM inbox_key WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let Some(sealed) = sealed else {
            return Ok(None);
        };
        let bytes = Zeroizing::new(
            decrypt(storage_key, &sealed).map_err(|e| StorageError::Encryption(e.to_string()))?,
        );
        let bytes: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| StorageError::Encryption("inbox private key length".into()))?;
        Ok(Some(StaticSecret::from(bytes)))
    }

    /// Seals `update` to the inbox public key and adds it.
    pub fn put(&self, update: &[u8]) -> Result<(), StorageError> {
        let sealed = sealed_box::seal_with(derive_inbox_seal_key, update, &self.public_key()?)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO receive_inbox (sealed_update) VALUES (?1)",
            params![sealed],
        )?;
        Ok(())
    }

    /// Every item, oldest first, opened with the inbox private key.
    pub fn items(&self) -> Result<InboxItems, StorageError> {
        if self.storage_key.is_none() {
            return Err(StorageError::StorageKeyUnavailable);
        }
        let rows: Vec<(i64, Vec<u8>)> = self
            .conn
            .prepare("SELECT id, sealed_update FROM receive_inbox ORDER BY id")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let mut items = InboxItems {
            readable: Vec::new(),
            unreadable: Vec::new(),
        };
        if rows.is_empty() {
            return Ok(items);
        }
        let Some(private_key) = self.private_key()? else {
            items.unreadable = rows.into_iter().map(|(id, _)| id).collect();
            return Ok(items);
        };
        for (id, sealed) in rows {
            match sealed_box::open_with(derive_inbox_seal_key, &sealed, &private_key) {
                Ok(update) => items.readable.push(InboxItem {
                    id,
                    update: Zeroizing::new(update),
                }),
                Err(_) => items.unreadable.push(id),
            }
        }
        Ok(items)
    }

    /// Removes the item `id`, once merged or found unreadable.
    pub fn remove(&self, id: i64) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM receive_inbox WHERE id = ?1", params![id])?;
        Ok(())
    }
}
