// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A batch advances exactly the state its caller read, not a later cached read.

use super::*;

/// One observation, including absence, bound to its contact, kind and device.
/// The private ciphertext and version identify this exact stored generation.
pub struct NoticeSnapshot {
    contact_id: String,
    kind: NoticeStateKind,
    peer_device_id: [u8; 32],
    state: Option<Zeroizing<Vec<u8>>>,
    encrypted: Vec<u8>,
    version: i64,
}

impl NoticeSnapshot {
    pub fn state(&self) -> Option<&[u8]> {
        self.state.as_deref().map(Vec::as_slice)
    }
}

/// An advanced state paired with the exact observation it was computed from.
pub struct NoticeWrite<'a> {
    observed: &'a NoticeSnapshot,
    state: &'a [u8],
}

impl<'a> NoticeWrite<'a> {
    pub fn new(observed: &'a NoticeSnapshot, state: &'a [u8]) -> Self {
        Self { observed, state }
    }
}

impl NoticeStore<'_> {
    /// Reads a state and its comparison token together, without changing a
    /// shared last-read cache. An absent state is a create-only observation.
    pub fn snapshot(
        &self,
        contact_id: &str,
        kind: NoticeStateKind,
        peer_device_id: &[u8; 32],
    ) -> Result<NoticeSnapshot, StorageError> {
        let row: Option<(Vec<u8>, bool, i64)> = self
            .conn
            .query_row(
                "SELECT state_encrypted, sealed_with_receive_key, row_version
             FROM notice_states
             WHERE contact_id = ?1 AND kind = ?2 AND peer_device_id = ?3",
                params![contact_id, kind.code(), peer_device_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let mut observed = NoticeSnapshot {
            contact_id: contact_id.to_string(),
            kind,
            peer_device_id: *peer_device_id,
            state: None,
            encrypted: Vec::new(),
            version: -1,
        };
        if let Some((encrypted, sealed_with_receive_key, version)) = row {
            let key = self.opening_key(contact_id, sealed_with_receive_key)?;
            observed.state = Some(Zeroizing::new(
                crate::crypto::decrypt(&key, &encrypted)
                    .map_err(|error| StorageError::Encryption(error.to_string()))?,
            ));
            observed.encrypted = encrypted;
            observed.version = version;
        }
        Ok(observed)
    }

    /// Commits every advanced state durably or refuses the entire batch.
    /// A newer read through the same connection cannot authorize an old copy.
    pub fn save_many(&self, writes: &[NoticeWrite<'_>]) -> Result<(), StorageError> {
        self.storage.with_notice_transaction(|| {
            for write in writes {
                let observed = write.observed;
                self.save_observed(
                    &observed.contact_id,
                    observed.kind,
                    &observed.peer_device_id,
                    write.state,
                    observed.version,
                    &observed.encrypted,
                )?;
            }
            Ok(())
        })
    }
}
