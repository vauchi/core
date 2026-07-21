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
    #[allow(dead_code)]
    conn: &'a Connection,
    #[allow(dead_code)]
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
        _contact_id: &str,
        _nonce: &[u8; 32],
        _signed_payload: &[u8],
        _received_at: u64,
    ) -> Result<bool, StorageError> {
        Err(StorageError::Serialization(
            "unimplemented: safety_alert_facts (RED)".into(),
        ))
    }

    /// All facts not yet surfaced to the presentation pipeline, oldest first.
    pub fn load_unsurfaced_facts(&self) -> Result<Vec<StoredSafetyAlertFact>, StorageError> {
        Err(StorageError::Serialization(
            "unimplemented: safety_alert_facts (RED)".into(),
        ))
    }

    /// Record that the fact was durably handed to the presentation pipeline.
    pub fn mark_fact_surfaced(
        &self,
        _contact_id: &str,
        _nonce: &[u8; 32],
        _surfaced_at: u64,
    ) -> Result<(), StorageError> {
        Err(StorageError::Serialization(
            "unimplemented: safety_alert_facts (RED)".into(),
        ))
    }
}
