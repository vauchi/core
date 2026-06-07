// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Re-encryption (rekey) of all encrypted columns when the storage encryption key changes.
//!
//! The [`ENCRYPTED_COLUMNS`] registry lists every `(table, column)` pair that rekey must
//! handle. The PRAGMA-based exhaustiveness test in `rekey_coverage_tests.rs` discovers all
//! `_encrypted`/`_hmac`/`encrypted_blob` columns from the live schema and asserts they
//! appear in this registry. Adding a new encrypted column to a migration without updating
//! this registry will fail that test.
//!
//! ## Structure
//!
//! [`Storage::rekey_with_progress`] is a thin orchestrator: it opens one EXCLUSIVE
//! transaction, then calls one `rekey_<table>` helper per encrypted table in sequence,
//! reporting progress after each. Each helper re-encrypts a single table's encrypted
//! columns and is independently testable. Keeping the per-table SQL in its own method
//! (rather than a generic dynamic-SQL helper) preserves auditability: every statement is
//! a static, greppable literal.

use hmac::{Hmac, Mac};
use rusqlite::params;
use sha2::Sha256;

use crate::crypto::encryption::EncryptionError;
use crate::crypto::{SymmetricKey, decrypt, encrypt, kdf::HKDF};

type HmacSha256 = Hmac<Sha256>;

use super::{Storage, StorageError};

/// Exhaustive registry of every encrypted column handled by rekey.
///
/// The PRAGMA-based test in `rekey_coverage_tests.rs` compares this list against the live
/// schema. If a migration adds a new `_encrypted`/`_hmac`/`encrypted_blob` column and this
/// registry is not updated, that test fails with a clear message.
///
/// Columns intentionally skipped (with documented reason) go in [`REKEY_SKIP_COLUMNS`].
#[cfg(feature = "testing")]
pub const ENCRYPTED_COLUMNS: &[(&str, &str)] = &[
    // V1 baseline
    ("contacts", "card_encrypted"),
    ("contacts", "shared_key_encrypted"),
    ("identity", "backup_data_encrypted"),
    ("contact_ratchets", "ratchet_state_encrypted"),
    // V4 contact extras
    ("contacts", "personal_notes_encrypted"),
    ("contacts", "avatar_encrypted"),
    // V32 contact field notes
    ("contact_field_notes", "note_encrypted"),
    // V13 crypto-shredding
    ("contacts", "cek_encrypted"),
    // V14 high-priority
    ("own_card", "card_json_encrypted"),
    ("device_registry", "registry_json_encrypted"),
    ("device_sync_state", "state_json_encrypted"),
    ("visibility_labels", "contacts_json_encrypted"),
    ("visibility_labels", "visible_fields_json_encrypted"),
    // V15 medium-priority
    ("device_info", "device_info_encrypted"),
    ("version_vector", "vector_json_encrypted"),
    ("contact_sync_timestamps", "last_sync_at_encrypted"),
    ("pending_updates", "payload_encrypted"),
    ("retry_entries", "payload_encrypted"),
    ("device_sync_checkpoints", "items_json_encrypted"),
    ("recovery_responses", "response_encrypted"),
    ("deletion_state", "state_json_encrypted"),
    ("sync_checkpoints", "state_json_encrypted"),
    // V16 low-priority
    ("field_validations", "field_value_encrypted"),
    ("field_validations", "signature_encrypted"),
    ("ux_state", "aha_tracker_json_encrypted"),
    ("ux_state", "demo_contact_json_encrypted"),
    ("audit_log", "details_encrypted"),
    // V18 visibility rules
    ("contacts", "visibility_rules_encrypted"),
    // V19 app password / duress
    ("identity", "password_hash_encrypted"),
    ("identity", "duress_hash_encrypted"),
    // V20 duress settings
    ("duress_settings", "alert_contact_ids_encrypted"),
    ("duress_settings", "alert_message_encrypted"),
    // V21 decoy contacts
    ("decoy_contacts", "card_encrypted"),
    // V22 emergency config
    ("emergency_config", "trusted_contact_ids_encrypted"),
    ("emergency_config", "message_encrypted"),
    // V23 label name encryption
    ("visibility_labels", "name_encrypted"),
    ("visibility_labels", "name_hmac"),
    // V26 recovery settings
    ("recovery_settings", "settings_encrypted"),
    // V29 onboarding progress
    ("ux_state", "onboarding_progress_encrypted"),
    // V44 backup reminder
    ("ux_state", "backup_reminder_encrypted"),
    // V30 label display name override
    ("visibility_labels", "display_name_override_encrypted"),
    // V38 exchange state crash recovery
    ("exchange_states", "encrypted_blob"),
    // V43 contact display: nickname, custom avatar, shared avatars
    ("contacts", "nickname_encrypted"),
    ("contacts", "custom_avatar_encrypted"),
    ("contact_shared_avatars", "avatar_encrypted"),
    // V44 in-progress recovery state
    ("recovery_progress", "progress_encrypted"),
    // V48 device-sync LWW field timestamps
    ("sync_field_timestamps", "timestamps_json_encrypted"),
    // V49 owner-private contact tags (ADR-051)
    ("tags", "name_encrypted"),
    // V50 owner-private named places (ADR-051)
    ("places", "data_encrypted"),
];

/// Encrypted columns intentionally skipped by rekey, with documented reason.
#[cfg(feature = "testing")]
pub const REKEY_SKIP_COLUMNS: &[(&str, &str, &str)] = &[
    // Deprecated in 2026-03-24, never written to, always NULL.
    (
        "ux_state",
        "tor_config_encrypted",
        "deprecated, always NULL",
    ),
];

/// Try to decrypt data with the old key; if decryption fails, treat it as
/// plaintext and encrypt directly with the new key. This self-heals columns
/// that were written as plaintext due to the V4/V32 encryption gap (columns
/// named `_encrypted` but never actually encrypted by callers).
///
/// Safety: valid ciphertexts start with algorithm tag 0x02 or 0x03. UTF-8 text
/// never starts with these bytes, so a decrypt failure unambiguously means the
/// data is plaintext.
fn rekey_or_heal(
    new_key: &SymmetricKey,
    old_key: &SymmetricKey,
    data: &[u8],
) -> Result<Vec<u8>, EncryptionError> {
    match decrypt(old_key, data) {
        Ok(plain) => encrypt(new_key, &plain),
        Err(_) => {
            // Data is plaintext (pre-encryption gap) — encrypt with new key.
            encrypt(new_key, data)
        }
    }
}

impl Storage {
    /// Re-encrypts all encrypted columns from the current key to a new key.
    ///
    /// This is used during SMK migration: the database was opened with the old
    /// storage_key, and all data needs to be re-encrypted with the new SEK
    /// derived from SMK. After successful rekey, the internal encryption_key
    /// is updated to the new key.
    ///
    /// The operation runs in a single transaction for atomicity — if any step
    /// fails, all changes are rolled back and the old key remains valid.
    ///
    /// The optional `progress` callback receives `(completed_tables, total_tables, table_name)`
    /// after each table is re-encrypted (#166a).
    pub fn rekey(&mut self, new_key: SymmetricKey) -> Result<(), StorageError> {
        self.rekey_with_progress(new_key, None)
    }

    /// Re-encrypts all encrypted columns with progress reporting (#166a).
    ///
    /// See [`Self::rekey`] for details. The `progress` callback, if provided,
    /// is called after each table completes with `(completed, total, table_name)`.
    ///
    /// This is a thin orchestrator: one EXCLUSIVE transaction wraps a sequence
    /// of `rekey_<table>` helpers, one per encrypted table. On any error the
    /// whole transaction rolls back and the old key remains valid.
    #[allow(clippy::type_complexity)]
    pub fn rekey_with_progress(
        &mut self,
        new_key: SymmetricKey,
        progress: Option<&dyn Fn(u32, u32, &str)>,
    ) -> Result<(), StorageError> {
        // Flush WAL before rekey to ensure all data is in the main DB file (#129)
        self.wal_checkpoint()?;

        let old_key = &self.encryption_key;
        const TOTAL_TABLES: u32 = 31;
        let mut completed: u32 = 0;

        let report = |completed: &mut u32, table: &str| {
            *completed += 1;
            if let Some(cb) = &progress {
                cb(*completed, TOTAL_TABLES, table);
            }
        };

        self.conn.execute_batch("BEGIN EXCLUSIVE TRANSACTION")?;

        let result = (|| -> Result<(), StorageError> {
            self.rekey_contacts(old_key, &new_key)?;
            report(&mut completed, "contacts");

            self.rekey_contact_extras(old_key, &new_key)?;
            report(&mut completed, "contact_extras");

            self.rekey_contact_field_notes(old_key, &new_key)?;
            report(&mut completed, "contact_field_notes");

            self.rekey_identity_backup(old_key, &new_key)?;
            report(&mut completed, "identity");

            self.rekey_ratchets(old_key, &new_key)?;
            report(&mut completed, "ratchets");

            self.rekey_own_card(old_key, &new_key)?;
            report(&mut completed, "own_card");

            self.rekey_device_registry(old_key, &new_key)?;
            report(&mut completed, "device_registry");

            self.rekey_device_sync_state(old_key, &new_key)?;
            report(&mut completed, "device_sync_state");

            self.rekey_visibility_labels(old_key, &new_key)?;
            report(&mut completed, "visibility_labels");

            self.rekey_device_info(old_key, &new_key)?;
            report(&mut completed, "device_info");

            self.rekey_version_vector(old_key, &new_key)?;
            report(&mut completed, "version_vector");

            self.rekey_field_timestamps(old_key, &new_key)?;
            report(&mut completed, "field_timestamps");

            self.rekey_sync_timestamps(old_key, &new_key)?;
            report(&mut completed, "sync_timestamps");

            self.rekey_pending_updates(old_key, &new_key)?;
            report(&mut completed, "pending_updates");

            self.rekey_retry_entries(old_key, &new_key)?;
            report(&mut completed, "retry_entries");

            self.rekey_device_sync_checkpoints(old_key, &new_key)?;
            report(&mut completed, "device_sync_checkpoints");

            self.rekey_recovery_responses(old_key, &new_key)?;
            report(&mut completed, "recovery_responses");

            self.rekey_deletion_state(old_key, &new_key)?;
            report(&mut completed, "deletion_state");

            self.rekey_sync_checkpoints(old_key, &new_key)?;
            report(&mut completed, "sync_checkpoints");

            self.rekey_field_validations(old_key, &new_key)?;
            report(&mut completed, "field_validations");

            self.rekey_ux_state(old_key, &new_key)?;
            report(&mut completed, "ux_state");

            self.rekey_audit_log(old_key, &new_key)?;
            report(&mut completed, "audit_log");

            self.rekey_contacts_crypto(old_key, &new_key)?;
            report(&mut completed, "contacts_crypto");

            self.rekey_identity_passwords(old_key, &new_key)?;
            report(&mut completed, "identity_passwords");

            self.rekey_duress_settings(old_key, &new_key)?;
            report(&mut completed, "duress_settings");

            self.rekey_decoy_contacts(old_key, &new_key)?;
            report(&mut completed, "decoy_contacts");

            self.rekey_emergency_config(old_key, &new_key)?;
            report(&mut completed, "emergency_config");

            self.rekey_recovery_settings(old_key, &new_key)?;
            report(&mut completed, "recovery_settings");

            self.rekey_recovery_progress(old_key, &new_key)?;
            report(&mut completed, "recovery_progress");

            self.rekey_exchange_states(old_key, &new_key)?;
            report(&mut completed, "exchange_states");

            self.rekey_contact_display(old_key, &new_key)?;
            report(&mut completed, "contact_display");

            self.rekey_shared_avatars(old_key, &new_key)?;
            report(&mut completed, "shared_avatars");

            self.rekey_tags(old_key, &new_key)?;
            report(&mut completed, "tags");

            self.rekey_places(old_key, &new_key)?;
            report(&mut completed, "places");

            Ok(())
        })();

        match result {
            Ok(()) => {
                self.conn.execute_batch("COMMIT")?;
                self.encryption_key = new_key;
                Ok(())
            }
            Err(e) => {
                // best-effort: we're already in the error path; if ROLLBACK
                // itself fails the transaction will be rolled back when the
                // connection drops anyway
                #[allow(clippy::let_underscore_must_use)]
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    // ---------------------------------------------------------------------
    // Per-table rekey helpers. Each re-encrypts one table's encrypted columns
    // from `old_key` to `new_key`. Called in sequence by `rekey_with_progress`
    // inside its EXCLUSIVE transaction.
    // ---------------------------------------------------------------------

    /// Re-encrypt contacts: card_encrypted and shared_key_encrypted
    fn rekey_contacts(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, card_encrypted, shared_key_encrypted FROM contacts")
            .map_err(|e| StorageError::Migration(format!("Read contacts: {}", e)))?;

        let rows: Vec<(String, Vec<u8>, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query contacts: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect contacts: {}", e)))?;

        for (id, card_enc, key_enc) in &rows {
            let card_plain = decrypt(old_key, card_enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt card {}: {}", id, e)))?;
            let key_plain = decrypt(old_key, key_enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt key {}: {}", id, e)))?;

            let card_new = encrypt(new_key, &card_plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt card {}: {}", id, e)))?;
            let key_new = encrypt(new_key, &key_plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt key {}: {}", id, e)))?;

            self.conn.execute(
                "UPDATE contacts SET card_encrypted = ?1, shared_key_encrypted = ?2 WHERE id = ?3",
                params![card_new, key_new, id],
            ).map_err(|e| StorageError::Migration(format!("Update contact {}: {}", id, e)))?;
        }
        Ok(())
    }

    /// Re-encrypt contacts: personal_notes_encrypted and avatar_encrypted (nullable)
    ///
    /// Self-healing: these columns may contain plaintext (pre-encryption gap from
    /// migration V4). Valid ciphertexts start with 0x02/0x03 algorithm tags; UTF-8
    /// text never does. If decrypt fails, the data is plaintext — encrypt it
    /// directly with the new key, healing the gap in-place.
    fn rekey_contact_extras(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, personal_notes_encrypted, avatar_encrypted FROM contacts WHERE personal_notes_encrypted IS NOT NULL OR avatar_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read contact extras: {}", e)))?;

        type ContactExtras = (String, Option<Vec<u8>>, Option<Vec<u8>>);
        let rows: Vec<ContactExtras> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query contact extras: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect contact extras: {}", e)))?;

        for (id, notes_enc, avatar_enc) in &rows {
            let notes_new =
                if let Some(enc) = notes_enc {
                    Some(rekey_or_heal(new_key, old_key, enc).map_err(|e| {
                        StorageError::Migration(format!("Rekey notes {}: {}", id, e))
                    })?)
                } else {
                    None
                };

            let avatar_new =
                if let Some(enc) = avatar_enc {
                    Some(rekey_or_heal(new_key, old_key, enc).map_err(|e| {
                        StorageError::Migration(format!("Rekey avatar {}: {}", id, e))
                    })?)
                } else {
                    None
                };

            self.conn.execute(
                "UPDATE contacts SET personal_notes_encrypted = ?1, avatar_encrypted = ?2 WHERE id = ?3",
                params![notes_new, avatar_new, id],
            ).map_err(|e| StorageError::Migration(format!("Update contact extras {}: {}", id, e)))?;
        }
        Ok(())
    }

    /// Re-encrypt contact_field_notes: note_encrypted (self-healing, same gap as above)
    fn rekey_contact_field_notes(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT contact_id, field_id, note_encrypted FROM contact_field_notes")
            .map_err(|e| StorageError::Migration(format!("Read field_notes: {}", e)))?;

        let rows: Vec<(String, String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query field_notes: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect field_notes: {}", e)))?;

        for (contact_id, field_id, enc) in &rows {
            let new_enc = rekey_or_heal(new_key, old_key, enc).map_err(|e| {
                StorageError::Migration(format!(
                    "Rekey field_note {}:{}: {}",
                    contact_id, field_id, e
                ))
            })?;
            self.conn.execute(
                "UPDATE contact_field_notes SET note_encrypted = ?1 WHERE contact_id = ?2 AND field_id = ?3",
                params![new_enc, contact_id, field_id],
            ).map_err(|e| StorageError::Migration(format!("Update field_note {}:{}: {}", contact_id, field_id, e)))?;
        }
        Ok(())
    }

    /// Re-encrypt tags: name_encrypted (owner-private tag vocabulary, ADR-051)
    fn rekey_tags(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name_encrypted FROM tags")
            .map_err(|e| StorageError::Migration(format!("Read tags: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query tags: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect tags: {}", e)))?;

        for (id, enc) in &rows {
            let new_enc = rekey_or_heal(new_key, old_key, enc)
                .map_err(|e| StorageError::Migration(format!("Rekey tag {}: {}", id, e)))?;
            self.conn
                .execute(
                    "UPDATE tags SET name_encrypted = ?1 WHERE id = ?2",
                    params![new_enc, id],
                )
                .map_err(|e| StorageError::Migration(format!("Update tag {}: {}", id, e)))?;
        }
        Ok(())
    }

    /// Re-encrypt places: data_encrypted (named-place vocabulary, ADR-051)
    fn rekey_places(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, data_encrypted FROM places")
            .map_err(|e| StorageError::Migration(format!("Read places: {}", e)))?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query places: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect places: {}", e)))?;
        for (id, enc) in &rows {
            let new_enc = rekey_or_heal(new_key, old_key, enc)
                .map_err(|e| StorageError::Migration(format!("Rekey place {}: {}", id, e)))?;
            self.conn
                .execute(
                    "UPDATE places SET data_encrypted = ?1 WHERE id = ?2",
                    params![new_enc, id],
                )
                .map_err(|e| StorageError::Migration(format!("Update place {}: {}", id, e)))?;
        }
        Ok(())
    }

    /// Re-encrypt identity: backup_data_encrypted
    fn rekey_identity_backup(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(i64, Vec<u8>), _> = self.conn.query_row(
            "SELECT id, backup_data_encrypted FROM identity WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        if let Ok((id, backup_enc)) = result {
            let plain = decrypt(old_key, &backup_enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt identity: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt identity: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE identity SET backup_data_encrypted = ?1 WHERE id = ?2",
                    params![new_enc, id],
                )
                .map_err(|e| StorageError::Migration(format!("Update identity: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt ratchet state with per-contact derived keys (#126)
    fn rekey_ratchets(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT contact_id, ratchet_state_encrypted FROM contact_ratchets")
            .map_err(|e| StorageError::Migration(format!("Read ratchets: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query ratchets: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect ratchets: {}", e)))?;

        for (contact_id, ratchet_enc) in &rows {
            // Decrypt with old per-contact key
            let mut old_info = b"vauchi-ratchet-storage-v1:".to_vec();
            old_info.extend_from_slice(contact_id.as_bytes());
            let old_derived = HKDF::derive_key(None, old_key.as_bytes(), &old_info);
            let old_ratchet_key = SymmetricKey::from_bytes(*old_derived);

            let plain = decrypt(&old_ratchet_key, ratchet_enc).map_err(|e| {
                StorageError::Migration(format!("Decrypt ratchet {}: {}", contact_id, e))
            })?;

            // Re-encrypt with new per-contact key
            let mut new_info = b"vauchi-ratchet-storage-v1:".to_vec();
            new_info.extend_from_slice(contact_id.as_bytes());
            let new_derived = HKDF::derive_key(None, new_key.as_bytes(), &new_info);
            let new_ratchet_key = SymmetricKey::from_bytes(*new_derived);

            let new_enc = encrypt(&new_ratchet_key, &plain).map_err(|e| {
                StorageError::Migration(format!("Encrypt ratchet {}: {}", contact_id, e))
            })?;
            self.conn.execute(
                "UPDATE contact_ratchets SET ratchet_state_encrypted = ?1 WHERE contact_id = ?2",
                params![new_enc, contact_id],
            ).map_err(|e| StorageError::Migration(format!("Update ratchet {}: {}", contact_id, e)))?;
        }
        Ok(())
    }

    /// Re-encrypt own_card: card_json_encrypted
    fn rekey_own_card(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT card_json_encrypted FROM own_card WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt own_card: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt own_card: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE own_card SET card_json_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update own_card: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt device_registry: registry_json_encrypted
    fn rekey_device_registry(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT registry_json_encrypted FROM device_registry WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt registry: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt registry: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE device_registry SET registry_json_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update registry: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt device_sync_state: state_json_encrypted
    fn rekey_device_sync_state(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT device_id, state_json_encrypted FROM device_sync_state WHERE state_json_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read device_sync: {}", e)))?;

        let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query device_sync: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect device_sync: {}", e)))?;

        for (device_id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt device_sync: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt device_sync: {}", e)))?;
                self.conn.execute(
                    "UPDATE device_sync_state SET state_json_encrypted = ?1 WHERE device_id = ?2",
                    params![new_enc, device_id],
                ).map_err(|e| StorageError::Migration(format!("Update device_sync: {}", e)))?;
            }
        }
        Ok(())
    }

    /// Re-encrypt visibility_labels: contacts_json, visible_fields_json, name, name_hmac, display_name_override
    fn rekey_visibility_labels(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, contacts_json_encrypted, visible_fields_json_encrypted, name_encrypted, display_name_override_encrypted FROM visibility_labels WHERE contacts_json_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read labels: {}", e)))?;

        #[allow(clippy::type_complexity)]
        let rows: Vec<(
            String,
            Vec<u8>,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
            Option<Vec<u8>>,
        )> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(|e| StorageError::Migration(format!("Query labels: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect labels: {}", e)))?;

        // Derive HMAC keys for old and new SEK
        let new_hmac_key_bytes =
            HKDF::derive_key(None, new_key.as_bytes(), b"Vauchi_Label_Name_HMAC_v1");
        let new_hmac_key_ref: &[u8] = &*new_hmac_key_bytes;

        for (id, contacts_enc, fields_enc, name_enc, override_enc) in &rows {
            let contacts_new = if !contacts_enc.is_empty() {
                let plain = decrypt(old_key, contacts_enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt label contacts {}: {}", id, e))
                })?;
                encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt label contacts {}: {}", id, e))
                })?
            } else {
                contacts_enc.clone()
            };

            let fields_new = if let Some(enc) = fields_enc {
                if !enc.is_empty() {
                    let plain = decrypt(old_key, enc).map_err(|e| {
                        StorageError::Migration(format!("Decrypt label fields {}: {}", id, e))
                    })?;
                    Some(encrypt(new_key, &plain).map_err(|e| {
                        StorageError::Migration(format!("Encrypt label fields {}: {}", id, e))
                    })?)
                } else {
                    Some(enc.clone())
                }
            } else {
                None
            };

            // Re-encrypt name and recompute HMAC (#128)
            let (name_new, name_hmac_new) = if let Some(enc) = name_enc {
                if !enc.is_empty() {
                    let plain = decrypt(old_key, enc).map_err(|e| {
                        StorageError::Migration(format!("Decrypt label name {}: {}", id, e))
                    })?;
                    let new_enc = encrypt(new_key, &plain).map_err(|e| {
                        StorageError::Migration(format!("Encrypt label name {}: {}", id, e))
                    })?;
                    let mut mac = HmacSha256::new_from_slice(new_hmac_key_ref)
                        .expect("HMAC accepts any key length");
                    mac.update(&plain);
                    let hmac_val = mac.finalize().into_bytes();
                    (Some(new_enc), Some(hmac_val.to_vec()))
                } else {
                    (Some(enc.clone()), None)
                }
            } else {
                (None, None)
            };

            let override_new = if let Some(enc) = override_enc {
                if !enc.is_empty() {
                    let plain = decrypt(old_key, enc).map_err(|e| {
                        StorageError::Migration(format!("Decrypt label override: {}", e))
                    })?;
                    Some(encrypt(new_key, &plain).map_err(|e| {
                        StorageError::Migration(format!("Encrypt label override: {}", e))
                    })?)
                } else {
                    Some(enc.clone())
                }
            } else {
                None
            };

            self.conn.execute(
                "UPDATE visibility_labels SET contacts_json_encrypted = ?1, visible_fields_json_encrypted = ?2, name_encrypted = ?3, name_hmac = ?4, display_name_override_encrypted = ?5 WHERE id = ?6",
                params![contacts_new, fields_new, name_new, name_hmac_new, override_new, id],
            ).map_err(|e| StorageError::Migration(format!("Update label: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt device_info: device_info_encrypted
    fn rekey_device_info(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT device_info_encrypted FROM device_info WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt device_info: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt device_info: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE device_info SET device_info_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update device_info: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt version_vector: vector_json_encrypted
    fn rekey_version_vector(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT vector_json_encrypted FROM version_vector WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt version_vector: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt version_vector: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE version_vector SET vector_json_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update version_vector: {}", e)))?;
        }
        Ok(())
    }

    fn rekey_field_timestamps(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT timestamps_json_encrypted FROM sync_field_timestamps WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc).map_err(|e| {
                StorageError::Migration(format!("Decrypt sync_field_timestamps: {}", e))
            })?;
            let new_enc = encrypt(new_key, &plain).map_err(|e| {
                StorageError::Migration(format!("Encrypt sync_field_timestamps: {}", e))
            })?;
            self.conn
                .execute(
                    "UPDATE sync_field_timestamps SET timestamps_json_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| {
                    StorageError::Migration(format!("Update sync_field_timestamps: {}", e))
                })?;
        }
        Ok(())
    }

    /// Re-encrypt contact_sync_timestamps: last_sync_at_encrypted
    fn rekey_sync_timestamps(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT contact_id, last_sync_at_encrypted FROM contact_sync_timestamps WHERE last_sync_at_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read sync_timestamps: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query sync_timestamps: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect sync_timestamps: {}", e)))?;

        for (contact_id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt sync_ts: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt sync_ts: {}", e)))?;
                self.conn.execute(
                    "UPDATE contact_sync_timestamps SET last_sync_at_encrypted = ?1 WHERE contact_id = ?2",
                    params![new_enc, contact_id],
                ).map_err(|e| StorageError::Migration(format!("Update sync_ts: {}", e)))?;
            }
        }
        Ok(())
    }

    /// Re-encrypt pending_updates: payload_encrypted
    fn rekey_pending_updates(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT id, payload_encrypted FROM pending_updates WHERE payload_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read pending: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query pending: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect pending: {}", e)))?;

        for (id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt pending {}: {}", id, e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt pending {}: {}", id, e))
                })?;
                self.conn
                    .execute(
                        "UPDATE pending_updates SET payload_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update pending {}: {}", id, e))
                    })?;
            }
        }
        Ok(())
    }

    /// Re-encrypt retry_entries: payload_encrypted
    fn rekey_retry_entries(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT message_id, payload_encrypted FROM retry_entries WHERE payload_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read retry: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query retry: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect retry: {}", e)))?;

        for (id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt retry {}: {}", id, e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt retry {}: {}", id, e)))?;
                self.conn
                    .execute(
                        "UPDATE retry_entries SET payload_encrypted = ?1 WHERE message_id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update retry {}: {}", id, e)))?;
            }
        }
        Ok(())
    }

    /// Re-encrypt device_sync_checkpoints: items_json_encrypted
    fn rekey_device_sync_checkpoints(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT target_device_id, items_json_encrypted FROM device_sync_checkpoints WHERE items_json_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read checkpoints: {}", e)))?;

        let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query checkpoints: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect checkpoints: {}", e)))?;

        for (device_id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt checkpoint: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt checkpoint: {}", e)))?;
                self.conn.execute(
                    "UPDATE device_sync_checkpoints SET items_json_encrypted = ?1 WHERE target_device_id = ?2",
                    params![new_enc, device_id],
                ).map_err(|e| StorageError::Migration(format!("Update checkpoint: {}", e)))?;
            }
        }
        Ok(())
    }

    /// Re-encrypt recovery_responses: response_encrypted
    fn rekey_recovery_responses(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT claim_id, response_encrypted FROM recovery_responses WHERE response_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read recovery: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query recovery: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect recovery: {}", e)))?;

        for (id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt recovery {}: {}", id, e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt recovery {}: {}", id, e))
                })?;
                self.conn
                    .execute(
                        "UPDATE recovery_responses SET response_encrypted = ?1 WHERE claim_id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update recovery {}: {}", id, e))
                    })?;
            }
        }
        Ok(())
    }

    /// Re-encrypt deletion_state: state_json_encrypted
    fn rekey_deletion_state(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT state_json_encrypted FROM deletion_state WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt deletion_state: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt deletion_state: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE deletion_state SET state_json_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update deletion_state: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt sync_checkpoints: state_json_encrypted
    fn rekey_sync_checkpoints(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT checkpoint_id, state_json_encrypted FROM sync_checkpoints WHERE state_json_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read batch_checkpoints: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query batch_checkpoints: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect batch_checkpoints: {}", e)))?;

        for (id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt batch_checkpoint {}: {}", id, e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt batch_checkpoint {}: {}", id, e))
                })?;
                self.conn.execute(
                    "UPDATE sync_checkpoints SET state_json_encrypted = ?1 WHERE checkpoint_id = ?2",
                    params![new_enc, id],
                ).map_err(|e| StorageError::Migration(format!("Update batch_checkpoint {}: {}", id, e)))?;
            }
        }
        Ok(())
    }

    /// Re-encrypt field_validations: field_value_encrypted, signature_encrypted
    fn rekey_field_validations(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, field_value_encrypted, signature_encrypted FROM field_validations")
            .map_err(|e| {
                StorageError::Migration(format!("Read field_validations for rekey: {}", e))
            })?;
        #[allow(clippy::type_complexity)]
        let rows: Vec<(String, Option<Vec<u8>>, Option<Vec<u8>>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query field_validations: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect field_validations: {}", e)))?;

        for (id, fv_enc, sig_enc) in &rows {
            if let Some(enc) = fv_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt field_value {}: {}", id, e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt field_value {}: {}", id, e))
                })?;
                self.conn
                    .execute(
                        "UPDATE field_validations SET field_value_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update field_value {}: {}", id, e))
                    })?;
            }
            if let Some(enc) = sig_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt signature {}: {}", id, e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt signature {}: {}", id, e))
                })?;
                self.conn
                    .execute(
                        "UPDATE field_validations SET signature_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update signature {}: {}", id, e))
                    })?;
            }
        }
        Ok(())
    }

    /// Re-encrypt ux_state: aha_tracker, demo_contact, onboarding_progress, backup_reminder
    fn rekey_ux_state(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result = self.conn.query_row(
            "SELECT id, aha_tracker_json_encrypted, demo_contact_json_encrypted, onboarding_progress_encrypted, backup_reminder_encrypted FROM ux_state WHERE id = 1",
            [],
            |row| {
                let id: i64 = row.get(0)?;
                let aha: Option<Vec<u8>> = row.get(1)?;
                let demo: Option<Vec<u8>> = row.get(2)?;
                let onboarding: Option<Vec<u8>> = row.get(3)?;
                let backup_reminder: Option<Vec<u8>> = row.get(4)?;
                Ok((id, aha, demo, onboarding, backup_reminder))
            },
        );

        if let Ok((id, aha_enc, demo_enc, onboarding_enc, backup_reminder_enc)) = result {
            if let Some(enc) = aha_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt aha_tracker: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt aha_tracker: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE ux_state SET aha_tracker_json_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update aha_tracker: {}", e)))?;
            }
            if let Some(enc) = demo_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt demo_contact: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt demo_contact: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE ux_state SET demo_contact_json_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update demo_contact: {}", e)))?;
            }
            if let Some(enc) = onboarding_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt onboarding: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt onboarding: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE ux_state SET onboarding_progress_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update onboarding: {}", e)))?;
            }
            if let Some(enc) = backup_reminder_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt backup_reminder: {}", e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt backup_reminder: {}", e))
                })?;
                self.conn
                    .execute(
                        "UPDATE ux_state SET backup_reminder_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update backup_reminder: {}", e))
                    })?;
            }
        }
        Ok(())
    }

    /// Re-encrypt audit_log: details_encrypted
    fn rekey_audit_log(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, details_encrypted FROM audit_log WHERE details_encrypted IS NOT NULL",
            )
            .map_err(|e| StorageError::Migration(format!("Read audit_log for rekey: {}", e)))?;
        let rows: Vec<(i64, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query audit_log: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect audit_log: {}", e)))?;

        for (id, enc) in &rows {
            if !enc.is_empty() {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt audit_log {}: {}", id, e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt audit_log {}: {}", id, e))
                })?;
                self.conn
                    .execute(
                        "UPDATE audit_log SET details_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update audit_log {}: {}", id, e))
                    })?;
            }
        }
        Ok(())
    }

    /// Re-encrypt contacts: cek_encrypted, visibility_rules_encrypted (nullable)
    fn rekey_contacts_crypto(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self.conn
            .prepare("SELECT id, cek_encrypted, visibility_rules_encrypted FROM contacts WHERE cek_encrypted IS NOT NULL OR visibility_rules_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read contact crypto: {}", e)))?;

        #[allow(clippy::type_complexity)]
        let rows: Vec<(String, Option<Vec<u8>>, Option<Vec<u8>>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query contact crypto: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect contact crypto: {}", e)))?;

        for (id, cek_enc, vis_enc) in &rows {
            if let Some(enc) = cek_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt cek: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt cek: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE contacts SET cek_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update cek: {}", e)))?;
            }
            if let Some(enc) = vis_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt visibility_rules: {}", e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt visibility_rules: {}", e))
                })?;
                self.conn
                    .execute(
                        "UPDATE contacts SET visibility_rules_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| {
                        StorageError::Migration(format!("Update visibility_rules: {}", e))
                    })?;
            }
        }
        Ok(())
    }

    /// Re-encrypt identity: password_hash_encrypted, duress_hash_encrypted (nullable)
    fn rekey_identity_passwords(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        #[allow(clippy::type_complexity)]
        let result: Result<(i64, Option<Vec<u8>>, Option<Vec<u8>>), _> = self.conn.query_row(
            "SELECT id, password_hash_encrypted, duress_hash_encrypted FROM identity WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        );

        if let Ok((id, pw_enc, duress_enc)) = result {
            if let Some(enc) = pw_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt password_hash: {}", e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt password_hash: {}", e))
                })?;
                self.conn
                    .execute(
                        "UPDATE identity SET password_hash_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update password_hash: {}", e)))?;
            }
            if let Some(enc) = duress_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt duress_hash: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt duress_hash: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE identity SET duress_hash_encrypted = ?1 WHERE id = ?2",
                        params![new_enc, id],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update duress_hash: {}", e)))?;
            }
        }
        Ok(())
    }

    /// Re-encrypt duress_settings: alert_contact_ids_encrypted, alert_message_encrypted
    fn rekey_duress_settings(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        #[allow(clippy::type_complexity)]
        let result: Result<(Option<Vec<u8>>, Option<Vec<u8>>), _> = self.conn.query_row(
            "SELECT alert_contact_ids_encrypted, alert_message_encrypted FROM duress_settings WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        if let Ok((ids_enc, msg_enc)) = result {
            if let Some(enc) = ids_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt duress_ids: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt duress_ids: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE duress_settings SET alert_contact_ids_encrypted = ?1 WHERE id = 1",
                        params![new_enc],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update duress_ids: {}", e)))?;
            }
            if let Some(enc) = msg_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc)
                    .map_err(|e| StorageError::Migration(format!("Decrypt duress_msg: {}", e)))?;
                let new_enc = encrypt(new_key, &plain)
                    .map_err(|e| StorageError::Migration(format!("Encrypt duress_msg: {}", e)))?;
                self.conn
                    .execute(
                        "UPDATE duress_settings SET alert_message_encrypted = ?1 WHERE id = 1",
                        params![new_enc],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update duress_msg: {}", e)))?;
            }
        }
        // Table may not have a row — that's fine, query_row returns Err(QueryReturnedNoRows)
        Ok(())
    }

    /// Re-encrypt decoy_contacts: card_encrypted (multi-row)
    fn rekey_decoy_contacts(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, card_encrypted FROM decoy_contacts")
            .map_err(|e| StorageError::Migration(format!("Read decoys: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query decoys: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect decoys: {}", e)))?;

        for (id, enc) in &rows {
            let plain = decrypt(old_key, enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt decoy: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt decoy: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE decoy_contacts SET card_encrypted = ?1 WHERE id = ?2",
                    params![new_enc, id],
                )
                .map_err(|e| StorageError::Migration(format!("Update decoy: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt emergency_config: trusted_contact_ids_encrypted, message_encrypted
    fn rekey_emergency_config(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        #[allow(clippy::type_complexity)]
        let result: Result<(Option<Vec<u8>>, Option<Vec<u8>>), _> = self.conn.query_row(
            "SELECT trusted_contact_ids_encrypted, message_encrypted FROM emergency_config WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        if let Ok((ids_enc, msg_enc)) = result {
            if let Some(enc) = ids_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt emergency_ids: {}", e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt emergency_ids: {}", e))
                })?;
                self.conn.execute(
                        "UPDATE emergency_config SET trusted_contact_ids_encrypted = ?1 WHERE id = 1",
                        params![new_enc],
                    ).map_err(|e| StorageError::Migration(format!("Update emergency_ids: {}", e)))?;
            }
            if let Some(enc) = msg_enc
                && !enc.is_empty()
            {
                let plain = decrypt(old_key, &enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt emergency_msg: {}", e))
                })?;
                let new_enc = encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt emergency_msg: {}", e))
                })?;
                self.conn
                    .execute(
                        "UPDATE emergency_config SET message_encrypted = ?1 WHERE id = 1",
                        params![new_enc],
                    )
                    .map_err(|e| StorageError::Migration(format!("Update emergency_msg: {}", e)))?;
            }
        }
        Ok(())
    }

    /// Re-encrypt recovery_settings: settings_encrypted (singleton)
    fn rekey_recovery_settings(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT settings_encrypted FROM recovery_settings WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc).map_err(|e| {
                StorageError::Migration(format!("Decrypt recovery_settings: {}", e))
            })?;
            let new_enc = encrypt(new_key, &plain).map_err(|e| {
                StorageError::Migration(format!("Encrypt recovery_settings: {}", e))
            })?;
            self.conn
                .execute(
                    "UPDATE recovery_settings SET settings_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update recovery_settings: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt recovery_progress: progress_encrypted (singleton, V44)
    fn rekey_recovery_progress(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let result: Result<(Option<Vec<u8>>,), _> = self.conn.query_row(
            "SELECT progress_encrypted FROM recovery_progress WHERE id = 1",
            [],
            |row| Ok((row.get(0)?,)),
        );

        if let Ok((Some(enc),)) = result
            && !enc.is_empty()
        {
            let plain = decrypt(old_key, &enc).map_err(|e| {
                StorageError::Migration(format!("Decrypt recovery_progress: {}", e))
            })?;
            let new_enc = encrypt(new_key, &plain).map_err(|e| {
                StorageError::Migration(format!("Encrypt recovery_progress: {}", e))
            })?;
            self.conn
                .execute(
                    "UPDATE recovery_progress SET progress_encrypted = ?1 WHERE id = 1",
                    params![new_enc],
                )
                .map_err(|e| StorageError::Migration(format!("Update recovery_progress: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt exchange_states: encrypted_blob (multi-row, crash recovery)
    fn rekey_exchange_states(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT exchange_id, encrypted_blob FROM exchange_states")
            .map_err(|e| StorageError::Migration(format!("Read exchange_states: {}", e)))?;

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| StorageError::Migration(format!("Query exchange_states: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect exchange_states: {}", e)))?;

        for (id, enc) in &rows {
            let plain = decrypt(old_key, enc)
                .map_err(|e| StorageError::Migration(format!("Decrypt exchange_state: {}", e)))?;
            let new_enc = encrypt(new_key, &plain)
                .map_err(|e| StorageError::Migration(format!("Encrypt exchange_state: {}", e)))?;
            self.conn
                .execute(
                    "UPDATE exchange_states SET encrypted_blob = ?1 WHERE exchange_id = ?2",
                    params![new_enc, id],
                )
                .map_err(|e| StorageError::Migration(format!("Update exchange_state: {}", e)))?;
        }
        Ok(())
    }

    /// Re-encrypt contacts: nickname_encrypted, custom_avatar_encrypted (nullable, V43)
    fn rekey_contact_display(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, nickname_encrypted, custom_avatar_encrypted FROM contacts WHERE nickname_encrypted IS NOT NULL OR custom_avatar_encrypted IS NOT NULL")
            .map_err(|e| StorageError::Migration(format!("Read contact_display: {}", e)))?;

        type ContactDisplay = (String, Option<Vec<u8>>, Option<Vec<u8>>);
        let rows: Vec<ContactDisplay> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query contact_display: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect contact_display: {}", e)))?;

        for (id, nick_enc, avatar_enc) in &rows {
            let nick_new = if let Some(enc) = nick_enc {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt nickname {}: {}", id, e))
                })?;
                Some(encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt nickname {}: {}", id, e))
                })?)
            } else {
                None
            };

            let avatar_new = if let Some(enc) = avatar_enc {
                let plain = decrypt(old_key, enc).map_err(|e| {
                    StorageError::Migration(format!("Decrypt custom_avatar {}: {}", id, e))
                })?;
                Some(encrypt(new_key, &plain).map_err(|e| {
                    StorageError::Migration(format!("Encrypt custom_avatar {}: {}", id, e))
                })?)
            } else {
                None
            };

            self.conn.execute(
                "UPDATE contacts SET nickname_encrypted = ?1, custom_avatar_encrypted = ?2 WHERE id = ?3",
                params![nick_new, avatar_new, id],
            ).map_err(|e| StorageError::Migration(format!("Update contact_display {}: {}", id, e)))?;
        }
        Ok(())
    }

    /// Re-encrypt contact_shared_avatars: avatar_encrypted (V43)
    fn rekey_shared_avatars(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT contact_id, avatar_hash, avatar_encrypted FROM contact_shared_avatars")
            .map_err(|e| StorageError::Migration(format!("Read shared_avatars: {}", e)))?;

        let rows: Vec<(String, String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| StorageError::Migration(format!("Query shared_avatars: {}", e)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| StorageError::Migration(format!("Collect shared_avatars: {}", e)))?;

        for (contact_id, avatar_hash, enc) in &rows {
            let plain = decrypt(old_key, enc).map_err(|e| {
                StorageError::Migration(format!(
                    "Decrypt shared_avatar {}/{}: {}",
                    contact_id, avatar_hash, e
                ))
            })?;
            let new_enc = encrypt(new_key, &plain).map_err(|e| {
                StorageError::Migration(format!(
                    "Encrypt shared_avatar {}/{}: {}",
                    contact_id, avatar_hash, e
                ))
            })?;
            self.conn.execute(
                "UPDATE contact_shared_avatars SET avatar_encrypted = ?1 WHERE contact_id = ?2 AND avatar_hash = ?3",
                params![new_enc, contact_id, avatar_hash],
            ).map_err(|e| StorageError::Migration(format!("Update shared_avatar {}/{}: {}", contact_id, avatar_hash, e)))?;
        }
        Ok(())
    }
}
