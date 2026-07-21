// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Received safety-alert fact persistence (`safety_alert_facts`).
//!
//! Append-only, nonce-keyed facts for verified duress/emergency alerts.
//! The receive path burns the replay nonce when it accepts an alert, so the
//! alert must be durable from the same transaction onward — an in-memory-only
//! alert is unrecoverable after a crash (delivery-axis findings,
//! `2026-07-21-per-device-ratchet-registry-dormant`). Facts store the exact
//! signed wire payload (encrypted at rest, ADR-015) so a sibling device can
//! re-verify the contact signature when fan-out ships.

use rusqlite::Connection;

use super::super::{Storage, StorageError};
use crate::crypto::SymmetricKey;

/// One durable, immutable received safety alert.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredSafetyAlertFact {
    pub contact_id: String,
    pub nonce: [u8; 32],
    /// Exact signed wire payload (`VersionedPayload` alert bytes), decrypted.
    pub signed_payload: Vec<u8>,
    pub received_at: u64,
}

/// Scoped persistence view for received safety-alert facts.
pub struct SafetyAlertFactStore<'a> {
    conn: &'a Connection,
    key: &'a SymmetricKey,
}

impl Storage {
    /// Scoped persistence view for received safety-alert facts.
    pub fn safety_alerts(&self) -> SafetyAlertFactStore<'_> {
        SafetyAlertFactStore {
            conn: &self.conn,
            key: &self.encryption_key,
        }
    }
}

impl SafetyAlertFactStore<'_> {
    /// Insert a verified alert fact; returns `true` if newly inserted,
    /// `false` if the `(contact_id, nonce)` fact already exists. An existing
    /// fact is never overwritten — facts are immutable.
    pub fn insert_fact_if_absent(
        &self,
        contact_id: &str,
        nonce: &[u8; 32],
        signed_payload: &[u8],
        received_at: u64,
    ) -> Result<bool, StorageError> {
        let payload_encrypted = crate::crypto::encrypt(self.key, signed_payload)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let changed = self.conn.execute(
            "INSERT OR IGNORE INTO safety_alert_facts
                 (contact_id, nonce, signed_payload_encrypted, received_at)
                 VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![contact_id, nonce, payload_encrypted, received_at as i64],
        )?;
        Ok(changed > 0)
    }

    /// All facts not yet surfaced to the presentation pipeline, oldest first.
    pub fn load_unsurfaced_facts(&self) -> Result<Vec<StoredSafetyAlertFact>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT contact_id, nonce, signed_payload_encrypted, received_at
                 FROM safety_alert_facts WHERE surfaced_at IS NULL
                 ORDER BY received_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;

        let mut facts = Vec::new();
        for row in rows {
            let (contact_id, nonce_bytes, payload_encrypted, received_at) = row?;
            // Per-row resilience: one corrupt row (bad nonce shape or
            // undecryptable payload) must never hide the healthy life-safety
            // alerts behind it. PII-free warn, then keep going.
            let Ok(nonce) = <[u8; 32]>::try_from(nonce_bytes) else {
                tracing::warn!("safety-alert fact row has a malformed nonce — skipping");
                continue;
            };
            let Ok(signed_payload) = crate::crypto::decrypt(self.key, &payload_encrypted) else {
                tracing::warn!("safety-alert fact payload failed to decrypt — skipping");
                continue;
            };
            facts.push(StoredSafetyAlertFact {
                contact_id,
                nonce,
                signed_payload,
                received_at: received_at as u64,
            });
        }
        Ok(facts)
    }

    /// Record that the fact was durably acknowledged by the presentation
    /// pipeline. Idempotent — marking an already-surfaced fact is a no-op.
    ///
    /// WHY nothing calls this in production yet: dispatch is not a truthful
    /// acknowledgement (the activity writer is channel-deferred, and the OS
    /// notification may never be scheduled). Marking at dispatch would
    /// recreate the crash-loss window one layer up. Call this only from the
    /// future durable presentation-acknowledgement contract
    /// (2026-07-21-per-device-ratchet-registry-dormant, delivery-axis).
    pub fn mark_fact_surfaced(
        &self,
        contact_id: &str,
        nonce: &[u8; 32],
        surfaced_at: u64,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE safety_alert_facts SET surfaced_at = ?3
                 WHERE contact_id = ?1 AND nonce = ?2 AND surfaced_at IS NULL",
            rusqlite::params![contact_id, nonce, surfaced_at as i64],
        )?;
        Ok(())
    }
}
