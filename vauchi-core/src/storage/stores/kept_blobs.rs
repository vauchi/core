// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The blobs a locked run kept (vauchi/private#1093). A kept blob's session
//! has moved past it, so when its ack is lost and the relay hands it out
//! again, trying it fails, and on a device session that failure deletes the
//! session. The receive paths acknowledge a recorded blob instead. Each is
//! recorded by a tag under the Background Receive Key, so a copy of the file
//! does not list which blobs arrived.

use rusqlite::{Connection, OptionalExtension, params};

use crate::crypto::{SymmetricKey, kdf::HKDF};

use super::super::{Storage, StorageError};

/// The relay keeps a blob 120 days; a record older than that has no blob
/// left to meet.
const KEPT_FOR_SECS: u64 = 120 * 86_400;

/// Scoped persistence view for the blobs a locked run kept.
pub struct KeptBlobStore<'a> {
    conn: &'a Connection,
    receive_key: Option<&'a SymmetricKey>,
}

impl Storage {
    /// Scoped persistence view for the blobs a locked run kept.
    pub fn kept_blobs(&self) -> crate::storage::Sealed<'_, KeptBlobStore<'_>> {
        self.sealed(KeptBlobStore::new(&self.conn, self.receive_key()))
    }
}

fn blob_tag(receive_key: &SymmetricKey, blob_id: &str) -> [u8; 32] {
    let mut info = b"vauchi-locked-kept-blob-v1:".to_vec();
    info.extend_from_slice(blob_id.as_bytes());
    *HKDF::derive_key(None, receive_key.as_bytes(), &info)
}

impl<'a> KeptBlobStore<'a> {
    pub(in crate::storage) fn new(
        conn: &'a Connection,
        receive_key: Option<&'a SymmetricKey>,
    ) -> Self {
        Self { conn, receive_key }
    }

    /// Records that a locked run kept `blob_id`, and forgets records older
    /// than the relay keeps a blob.
    pub fn record(&self, blob_id: &str, now: u64) -> Result<(), StorageError> {
        let key = self
            .receive_key
            .ok_or(StorageError::ReceiveKeyUnavailable)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO locked_kept_blobs (blob_tag, kept_at) VALUES (?1, ?2)",
            params![blob_tag(key, blob_id).as_slice(), now as i64],
        )?;
        self.conn.execute(
            "DELETE FROM locked_kept_blobs WHERE kept_at < ?1",
            params![now.saturating_sub(KEPT_FOR_SECS) as i64],
        )?;
        Ok(())
    }

    /// Whether a locked run kept `blob_id`. Without the receive key no
    /// locked run kept anything this storage can name.
    pub fn was_kept(&self, blob_id: &str) -> Result<bool, StorageError> {
        let Some(key) = self.receive_key else {
            return Ok(false);
        };
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM locked_kept_blobs WHERE blob_tag = ?1",
                params![blob_tag(key, blob_id).as_slice()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}
