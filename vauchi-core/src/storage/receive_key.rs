// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Background Receive Key in the storage file (ADR-033 amendment
//! 2026-11-07, vauchi/private#1093 slice 5). Receive state is sealed under
//! keys derived from it, in the same file as the cards it commits with, so
//! one transaction still covers both: SQLite in WAL mode makes a
//! transaction over two files atomic per file only.

use rusqlite::{OptionalExtension, params};
use zeroize::Zeroizing;

use crate::crypto::{SymmetricKey, decrypt, encrypt, kdf::HKDF};

use super::stores::ratchet::{ratchet_key_under_receive_key, ratchet_key_under_storage_key};
use super::{Storage, StorageError};

const CHECK_INFO: &[u8] = b"Vauchi_Receive_Key_Check_v1";
const CHECK_LABEL: &[u8] = b"vauchi receive key check";
const MAX_SEALED_LEN: usize = 256;

fn check_key(receive_key: &SymmetricKey) -> SymmetricKey {
    SymmetricKey::from_bytes(*HKDF::derive_key(None, receive_key.as_bytes(), CHECK_INFO))
}

impl Storage {
    /// Gives this storage the Background Receive Key. Receive state still
    /// under the storage key moves under it, in one transaction.
    ///
    /// Fails with [`StorageError::WrongKey`] when the file's receive state is
    /// under a different key; the storage then keeps none.
    pub fn set_receive_key(&mut self, receive_key: SymmetricKey) -> Result<(), StorageError> {
        let sealed: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT sealed_label FROM receive_key_check WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(sealed) = sealed {
            let opens = sealed.len() <= MAX_SEALED_LEN
                && decrypt(&check_key(&receive_key), &sealed)
                    .is_ok_and(|label| label == CHECK_LABEL);
            if !opens {
                return Err(StorageError::WrongKey);
            }
        }
        self.begin_transaction()?;
        match self.seal_receive_state(&receive_key) {
            Ok(()) => self.commit()?,
            Err(e) => {
                self.rollback();
                return Err(e);
            }
        }
        self.receive_key = Some(receive_key);
        Ok(())
    }

    pub(super) fn receive_key(&self) -> Option<&SymmetricKey> {
        self.receive_key.as_ref()
    }

    fn seal_receive_state(&self, receive_key: &SymmetricKey) -> Result<(), StorageError> {
        let sealed_label = encrypt(&check_key(receive_key), CHECK_LABEL)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            "INSERT OR IGNORE INTO receive_key_check (id, sealed_label) VALUES (1, ?1)",
            params![sealed_label],
        )?;

        let rows: Vec<(String, Vec<u8>, Vec<u8>)> = self
            .conn
            .prepare(
                "SELECT contact_id, peer_device_id, ratchet_state_encrypted
                 FROM contact_ratchets WHERE sealed_with_receive_key = 0",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<_, _>>()?;
        for (contact_id, peer_device_id, under_storage_key) in rows {
            let state = Zeroizing::new(
                decrypt(
                    &ratchet_key_under_storage_key(&self.encryption_key, &contact_id),
                    &under_storage_key,
                )
                .map_err(|e| StorageError::Encryption(e.to_string()))?,
            );
            let under_receive_key = encrypt(
                &ratchet_key_under_receive_key(receive_key, &contact_id),
                &state,
            )
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
            self.conn.execute(
                "UPDATE contact_ratchets
                 SET ratchet_state_encrypted = ?1, sealed_with_receive_key = 1
                 WHERE contact_id = ?2 AND peer_device_id = ?3",
                params![under_receive_key, contact_id, peer_device_id],
            )?;
        }
        Ok(())
    }
}
