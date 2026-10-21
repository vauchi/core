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

// Per-table rekey helpers, one `rekey_<table>` method each, grouped by domain.
// Each re-encrypts one table's encrypted columns from `old_key` to `new_key`;
// `rekey_with_progress` calls them in sequence inside its EXCLUSIVE transaction.
mod contacts;
mod identity;
mod safety;
mod sync;
mod visibility;

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
    ("contact_device_registries", "broadcast_encrypted"),
    // V62 received safety-alert facts
    ("safety_alert_facts", "signed_payload_encrypted"),
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
    // V21 decoy contacts
    ("decoy_contacts", "card_encrypted"),
    ("decoy_contacts", "display_name_encrypted"),
    ("contacts", "last_sent_display_name_encrypted"),
    ("contacts", "relay_url_encrypted"),
    ("contacts", "original_uid_encrypted"),
    ("contacts", "original_uid_hmac"),
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
    // V53 settings flags
    ("ux_state", "settings_flags_encrypted"),
    // V54 persisted relay URL
    ("ux_state", "relay_url_encrypted"),
    // V74 persisted relay's OHTTP anchor
    ("ux_state", "relay_anchor_encrypted"),
    // V84 this device's user-presence secret digest (#712)
    ("ux_state", "user_presence_digest_encrypted"),
    // V55 own-card repropagation marker
    ("ux_state", "own_card_repropagate_encrypted"),
    // V58 last-used exchange defaults (M2 S1)
    ("ux_state", "exchange_defaults_encrypted"),
    // V30 label display name override
    ("visibility_labels", "display_name_override_encrypted"),
    // V56 per-group presentation overrides (ADR-054 D2)
    ("visibility_labels", "bio_override_encrypted"),
    ("visibility_labels", "avatar_override_encrypted"),
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
    // V51 per-contact exchange location (ADR-051)
    ("contacts", "exchange_location_encrypted"),
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

/// Try to decrypt data with the old key; if it is not a ciphertext at all,
/// treat it as plaintext and encrypt directly with the new key. This
/// self-heals columns that were written as plaintext due to the V4/V32
/// encryption gap (columns named `_encrypted` but never actually encrypted by
/// callers).
///
/// A value shaped like a ciphertext (algorithm tag 0x02 or 0x03, long enough
/// for nonce and tag) that does not decrypt is a wrong-key or AD-bound
/// ciphertext, not gap plaintext: healing it would wrap it in a second layer
/// that never decrypts back to the data, so the error is returned instead
/// (vauchi/private#286).
fn rekey_or_heal(
    new_key: &SymmetricKey,
    old_key: &SymmetricKey,
    data: &[u8],
) -> Result<Vec<u8>, EncryptionError> {
    match decrypt(old_key, data) {
        Ok(plain) => encrypt(new_key, &plain),
        Err(e) if crate::crypto::encryption::looks_like_ciphertext(data) => Err(e),
        Err(_) => encrypt(new_key, data),
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
        const TOTAL_TABLES: u32 = 32;
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

            self.rekey_contact_device_registries(old_key, &new_key)?;
            report(&mut completed, "contact_device_registries");

            self.rekey_safety_alert_facts(old_key, &new_key)?;
            report(&mut completed, "safety_alert_facts");

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
            self.rekey_user_presence_digest(old_key, &new_key)?;
            report(&mut completed, "ux_state");

            self.rekey_audit_log(old_key, &new_key)?;
            report(&mut completed, "audit_log");

            self.rekey_contacts_crypto(old_key, &new_key)?;
            report(&mut completed, "contacts_crypto");

            self.rekey_identity_passwords(old_key, &new_key)?;
            report(&mut completed, "identity_passwords");

            self.rekey_decoy_contacts(old_key, &new_key)?;
            self.rekey_nullable_column(
                old_key,
                &new_key,
                "decoy_contacts",
                "display_name_encrypted",
            )?;
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

            self.rekey_exchange_location(old_key, &new_key)?;
            self.rekey_nullable_column(
                old_key,
                &new_key,
                "contacts",
                "last_sent_display_name_encrypted",
            )?;
            self.rekey_nullable_column(old_key, &new_key, "contacts", "relay_url_encrypted")?;
            self.rekey_contact_vcard_uids(old_key, &new_key)?;
            report(&mut completed, "exchange_location");

            super::key_check::rewrite(&self.conn, &new_key)
        })()
        .and_then(|()| self.commit());

        match result {
            Ok(()) => {
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
}
