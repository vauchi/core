// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 activation-handshake persistence view (migration v65).
//!
//! One row per contact snapshots the
//! [`ActivationTracker`](crate::sync::registry_activation::ActivationTracker);
//! a missing row rehydrates as the tracker default (`Dormant` — the legacy
//! `[0;32]` send path, i.e. exactly the pre-F4 shipped behavior, which is the
//! rollback guarantee for the activation slices). The row holds no secret
//! material — push nonces are correlation values and versions are counters —
//! so this store needs no encryption key, matching
//! `genesis_decrypt_contact_limits`.

use rusqlite::{OptionalExtension, params};

use super::super::{Storage, StorageError};
use crate::clock::Clock;
use crate::sync::registry_activation::ActivationTracker;
use std::sync::Arc;

/// Raw `registry_activation` row: `(push_nonce, pushed_version,
/// our_version_acked, peer_version_held)`.
type ActivationRow = (Option<Vec<u8>>, Option<i64>, Option<i64>, Option<i64>);

/// Scoped persistence view for F4 registry-activation handshake state.
pub struct RegistryActivationStore<'a> {
    conn: &'a rusqlite::Connection,
    clock: &'a Arc<dyn Clock>,
}

impl Storage {
    /// Scoped persistence view for F4 registry-activation handshake state.
    pub fn registry_activation(&self) -> RegistryActivationStore<'_> {
        RegistryActivationStore {
            conn: &self.conn,
            clock: &self.clock,
        }
    }
}

impl RegistryActivationStore<'_> {
    /// Snapshot `tracker` for `contact_id`, replacing any previous row.
    pub fn save_activation(
        &self,
        contact_id: &str,
        tracker: &ActivationTracker,
    ) -> Result<(), StorageError> {
        let (push_nonce, pushed_version) = match tracker.outstanding_push() {
            Some((nonce, version)) => (Some(nonce.to_vec()), Some(version as i64)),
            None => (None, None),
        };
        self.conn.execute(
            "INSERT INTO registry_activation
                 (contact_id, push_nonce, pushed_version, our_version_acked,
                  peer_version_held, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(contact_id) DO UPDATE SET
                     push_nonce = ?2, pushed_version = ?3,
                     our_version_acked = ?4, peer_version_held = ?5,
                     updated_at = ?6",
            params![
                contact_id,
                push_nonce,
                pushed_version,
                tracker.our_version_acked().map(|v| v as i64),
                tracker.peer_version_held().map(|v| v as i64),
                self.clock.unix_seconds() as i64,
            ],
        )?;
        Ok(())
    }

    /// All contacts' activation snapshots (device-link full-sync source).
    pub fn list_activations(&self) -> Result<Vec<(String, ActivationTracker)>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT contact_id FROM registry_activation")?;
        let ids: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        let mut out = Vec::with_capacity(ids.len());
        for contact_id in ids {
            if let Some(tracker) = self.load_activation(&contact_id)? {
                out.push((contact_id, tracker));
            }
        }
        Ok(out)
    }

    /// Rehydrate the tracker for `contact_id`; `None` when no handshake has
    /// been recorded (callers treat that as `Dormant`).
    pub fn load_activation(
        &self,
        contact_id: &str,
    ) -> Result<Option<ActivationTracker>, StorageError> {
        let row: Option<ActivationRow> = self
            .conn
            .query_row(
                "SELECT push_nonce, pushed_version, our_version_acked, peer_version_held
                     FROM registry_activation WHERE contact_id = ?1",
                params![contact_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((push_nonce, pushed_version, our_version_acked, peer_version_held)) = row else {
            return Ok(None);
        };
        // This table is written only by this store with u64 inputs, so a
        // negative version is tampering or corruption — fail closed like
        // the half-present-push case, never silently clamp (review F4).
        let checked_version = |value: i64| -> Result<u64, StorageError> {
            u64::try_from(value).map_err(|_| {
                StorageError::InvalidData("registry_activation version is negative".into())
            })
        };
        let outstanding_push = match (push_nonce, pushed_version) {
            (Some(nonce_bytes), Some(version)) => {
                let nonce: [u8; 32] = nonce_bytes.try_into().map_err(|_| {
                    StorageError::InvalidData("registry_activation push_nonce length".into())
                })?;
                Some((nonce, checked_version(version)?))
            }
            // A half-present push (nonce without version or vice versa) is
            // tampering or corruption — fail closed rather than guess.
            (None, None) => None,
            _ => {
                return Err(StorageError::InvalidData(
                    "registry_activation push fields disagree".into(),
                ));
            }
        };
        Ok(Some(ActivationTracker::from_parts(
            outstanding_push,
            our_version_acked.map(checked_version).transpose()?,
            peer_version_held.map(checked_version).transpose()?,
        )))
    }
}
