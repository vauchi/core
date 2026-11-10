// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The inbox of updates a locked run received (ADR-033 addendum 2026-11-09,
//! point 1, vauchi/private#1093 slice 6). Each item is sealed to the inbox
//! public key with a fresh ephemeral key; the private key is under the
//! storage key. A locked phone can add items but not read them, so a copy of
//! it taken later does not hold the updates already received.
//!
//! The public key is in the file, so whoever can write the file can seal an
//! item to it. Each item therefore also carries an HMAC under a key derived
//! from the Background Receive Key, and an item without a valid one is not
//! read: forging one takes what a locked run holds.

use hmac::{Hmac, Mac};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use crate::crypto::{SymmetricKey, X3DHKeyPair, decrypt, encrypt, kdf::HKDF};
use crate::recovery::sealed_box;

use super::super::{Storage, StorageError};

const INBOX_SEAL_INFO: &[u8] = b"Vauchi_Receive_Inbox_Seal_v1";
const INBOX_MAC_INFO: &[u8] = b"Vauchi_Receive_Inbox_Mac_v1";
const MAC_LEN: usize = 32;

/// How many items the inbox holds until the unlock merges them. A locked run
/// that meets a full inbox leaves the blob on the relay for the unlock, so
/// the cap bounds the file, not what is received.
pub const RECEIVE_INBOX_CAPACITY: usize = 256;

fn derive_inbox_seal_key(ikm: &[u8]) -> Zeroizing<[u8; 32]> {
    HKDF::derive_key(None, ikm, INBOX_SEAL_INFO)
}

fn inbox_mac(receive_key: &SymmetricKey) -> Hmac<Sha256> {
    let key = HKDF::derive_key(None, receive_key.as_bytes(), INBOX_MAC_INFO);
    // INVARIANT: HMAC accepts a key of any length (RFC 2104).
    #[allow(clippy::expect_used)]
    <Hmac<Sha256> as Mac>::new_from_slice(key.as_ref()).expect("HMAC takes any key length")
}

fn tagged(receive_key: &SymmetricKey, sealed: Vec<u8>) -> Vec<u8> {
    let mut mac = inbox_mac(receive_key);
    mac.update(&sealed);
    let mut out = sealed;
    out.extend_from_slice(&mac.finalize().into_bytes());
    out
}

/// The sealed part of `stored`, when its tag verifies under `receive_key`.
fn verified<'s>(receive_key: &SymmetricKey, stored: &'s [u8]) -> Option<&'s [u8]> {
    let (sealed, tag) = stored.split_at_checked(stored.len().checked_sub(MAC_LEN)?)?;
    let mut mac = inbox_mac(receive_key);
    mac.update(sealed);
    mac.verify_slice(tag).ok()?;
    Some(sealed)
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
    receive_key: Option<&'a SymmetricKey>,
}

impl<'a> ReceiveInboxStore<'a> {
    /// The view a locked run has: it can put items and read none.
    pub(in crate::storage) fn write_only(
        conn: &'a Connection,
        receive_key: &'a SymmetricKey,
    ) -> Self {
        Self {
            conn,
            storage_key: None,
            receive_key: Some(receive_key),
        }
    }
}

impl Storage {
    /// Scoped persistence view for the receive inbox.
    pub fn receive_inbox(&self) -> crate::storage::Sealed<'_, ReceiveInboxStore<'_>> {
        self.sealed(ReceiveInboxStore {
            conn: &self.conn,
            storage_key: Some(&self.encryption_key),
            receive_key: self.receive_key(),
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

    /// Seals `update` to the inbox public key, tags it under the receive
    /// key and adds it; a full inbox is [`StorageError::ReceiveInboxFull`].
    pub fn put(&self, update: &[u8]) -> Result<(), StorageError> {
        let receive_key = self
            .receive_key
            .ok_or(StorageError::ReceiveKeyUnavailable)?;
        let held: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM receive_inbox", [], |row| row.get(0))?;
        if usize::try_from(held).unwrap_or(usize::MAX) >= RECEIVE_INBOX_CAPACITY {
            return Err(StorageError::ReceiveInboxFull);
        }
        let sealed = sealed_box::seal_with(derive_inbox_seal_key, update, &self.public_key()?)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO receive_inbox (sealed_update) VALUES (?1)",
            params![tagged(receive_key, sealed)],
        )?;
        Ok(())
    }

    /// Every item, oldest first, opened with the inbox private key. Without
    /// the receive key no tag verifies, so every item is unreadable.
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
        let (Some(receive_key), Some(private_key)) = (self.receive_key, self.private_key()?) else {
            items.unreadable = rows.into_iter().map(|(id, _)| id).collect();
            return Ok(items);
        };
        for (id, stored) in rows {
            let opened = verified(receive_key, &stored).and_then(|sealed| {
                sealed_box::open_with(derive_inbox_seal_key, sealed, &private_key).ok()
            });
            match opened {
                Some(update) => items.readable.push(InboxItem {
                    id,
                    update: Zeroizing::new(update),
                }),
                None => items.unreadable.push(id),
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
