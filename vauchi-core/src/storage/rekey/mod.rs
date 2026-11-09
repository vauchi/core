// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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
use rusqlite::{OptionalExtension, params};
use sha2::Sha256;

use crate::crypto::encryption::EncryptionError;
use crate::crypto::{SymmetricKey, decrypt, encrypt, kdf::HKDF};

type HmacSha256 = Hmac<Sha256>;

use super::{Storage, StorageError};

// Per-table rekey helpers, one `rekey_<table>` method each, grouped by domain.
// Each re-encrypts one table's encrypted columns from `old_key` to `new_key`;
// `rekey_with_progress` calls them in sequence inside its EXCLUSIVE transaction.
mod contacts;
mod guardian;
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
    // Only a mask profile has these, beside its regular schema (ADR-080).
    ("mask_seed", "seed_encrypted"),
    ("decoy_ids", "display_name_encrypted"),
    ("public_values", "signing_public_key_encrypted"),
    ("public_values", "display_name_encrypted"),
    ("device_registry", "registry_json_encrypted"),
    ("contact_device_registries", "broadcast_encrypted"),
    // V62 received safety-alert facts
    ("safety_alert_facts", "signed_payload_encrypted"),
    ("device_sync_state", "state_json_encrypted"),
    ("visibility_labels", "contacts_json_encrypted"),
    ("visibility_labels", "visible_fields_json_encrypted"),
    // V15 medium-priority
    ("device_info", "device_info_encrypted"),
    // V113 per-device signing key
    ("device_signing_key", "key_encrypted"),
    ("version_vector", "vector_json_encrypted"),
    ("receive_checkpoints", "checkpoint_json_encrypted"),
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
    ("peer_card_ordering", "state_encrypted"),
    // V92 a revoked contact's name stays with its tombstone (#935)
    ("revoked_senders", "display_name_encrypted"),
    // V95 guardian backups kept for designators (#796)
    ("guardian_backup_pieces", "piece_encrypted"),
    ("guardian_backups", "delivery_encrypted"),
    // V97 backups guardians returned to a recovering identity (#796)
    ("returned_backup_pieces", "piece_encrypted"),
    ("returned_backups", "delivery_encrypted"),
    // V102 the owner's recovery passphrase key (#796)
    ("recovery_passphrase", "key_encrypted"),
    // V104 the recovery proof and rekey offers' exchange keys (#796)
    ("recovery_own_proof", "proof_encrypted"),
    ("recovery_rekey_offers", "exchange_secret_encrypted"),
    // V106 rekey answers waiting to be sent again (#796)
    ("recovery_rekey_answers", "answer_encrypted"),
    ("recovery_rekey_answers", "old_shared_encrypted"),
    // V107 the recovered identity's old seed (#796)
    ("recovery_old_identity", "seed_encrypted"),
    // V110 the temporary key of a phone in recovery mode and its
    // hand-backs (#796, P32)
    ("recovery_mode_key", "seed_encrypted"),
    ("recovery_hand_backs", "delivery_encrypted"),
    ("recovery_backup_pieces", "piece_encrypted"),
    // V49 owner-private contact tags (ADR-051)
    ("tags", "name_encrypted"),
    // V50 owner-private named places (ADR-051)
    ("places", "data_encrypted"),
    // V94 the sealed duress content, re-sealed whole (vauchi/private#1003)
    ("duress_content", "content_encrypted"),
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
    (
        "receive_tokens_ahead",
        "registration_encrypted",
        "sealed under the Background Receive Key, not the storage key (vauchi/private#1093)",
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
    fn table_conn(&self, _table: &str) -> &rusqlite::Connection {
        &self.conn
    }

    /// Re-encrypts the embedded mask profile: every table of its regular
    /// schema, and those only a mask profile has (ADR-080).
    fn rekey_mask(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        let Some(mask) = &self.mask else {
            return Ok(());
        };
        mask.rekey_tables(old_key, new_key, &mut |_| ())?;
        for (table, column) in [
            ("mask_seed", "seed_encrypted"),
            ("decoy_ids", "display_name_encrypted"),
            ("public_values", "signing_public_key_encrypted"),
            ("public_values", "display_name_encrypted"),
        ] {
            mask.rekey_nullable_column(old_key, new_key, table, column)?;
        }
        Ok(())
    }

    /// Re-encrypts the tables a legacy seal's twins were copied into, for a
    /// seal from before the mask key whose fields are under the SEK.
    pub(in crate::storage) fn rekey_mask_twins(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
    ) -> Result<(), StorageError> {
        self.rekey_tags(old_key, new_key, "tags")?;
        self.rekey_places(old_key, new_key, "places")?;
        self.rekey_visibility_labels(old_key, new_key, "visibility_labels")?;
        self.rekey_own_card(old_key, new_key, "own_card")?;
        self.rekey_emergency_config(old_key, new_key)
    }

    /// Re-encrypts every encrypted column of this database's tables from
    /// `old_key` to `new_key`, reporting each table as it is done.
    fn rekey_tables(
        &self,
        old_key: &SymmetricKey,
        new_key: &SymmetricKey,
        report: &mut dyn FnMut(&str),
    ) -> Result<(), StorageError> {
        self.rekey_contacts(old_key, new_key)?;
        report("contacts");

        self.rekey_contact_extras(old_key, new_key)?;
        report("contact_extras");

        self.rekey_contact_field_notes(old_key, new_key)?;
        report("contact_field_notes");

        self.rekey_identity_backup(old_key, new_key)?;
        report("identity");

        self.rekey_ratchets(old_key, new_key)?;
        report("ratchets");

        self.rekey_own_card(old_key, new_key, "own_card")?;
        report("own_card");

        self.rekey_device_registry(old_key, new_key)?;
        report("device_registry");

        self.rekey_contact_device_registries(old_key, new_key)?;
        report("contact_device_registries");

        self.rekey_safety_alert_facts(old_key, new_key)?;
        report("safety_alert_facts");

        self.rekey_device_sync_state(old_key, new_key)?;
        report("device_sync_state");

        self.rekey_visibility_labels(old_key, new_key, "visibility_labels")?;
        report("visibility_labels");

        self.rekey_device_info(old_key, new_key)?;
        report("device_info");

        self.rekey_device_signing_key(old_key, new_key)?;
        report("device_signing_key");

        self.rekey_version_vector(old_key, new_key)?;
        report("version_vector");

        self.rekey_receive_checkpoints(old_key, new_key)?;
        report("receive_checkpoints");

        self.rekey_field_timestamps(old_key, new_key)?;
        report("field_timestamps");

        self.rekey_nullable_column(old_key, new_key, "peer_card_ordering", "state_encrypted")?;
        report("peer_card_ordering");

        self.rekey_nullable_column(
            old_key,
            new_key,
            "revoked_senders",
            "display_name_encrypted",
        )?;
        report("revoked_senders");

        self.rekey_nullable_column(
            old_key,
            new_key,
            "guardian_backup_pieces",
            "piece_encrypted",
        )?;
        report("guardian_backup_pieces");

        self.rekey_nullable_column(old_key, new_key, "guardian_backups", "delivery_encrypted")?;
        report("guardian_backups");

        self.rekey_nullable_column(
            old_key,
            new_key,
            "returned_backup_pieces",
            "piece_encrypted",
        )?;
        report("returned_backup_pieces");

        self.rekey_nullable_column(old_key, new_key, "returned_backups", "delivery_encrypted")?;
        report("returned_backups");

        self.retag_guardian_designators(old_key, new_key)?;
        report("guardian_designator_tags");

        self.rekey_nullable_column(old_key, new_key, "recovery_passphrase", "key_encrypted")?;
        report("recovery_passphrase");

        self.rekey_nullable_column(old_key, new_key, "recovery_own_proof", "proof_encrypted")?;
        report("recovery_own_proof");

        self.rekey_nullable_column(
            old_key,
            new_key,
            "recovery_rekey_offers",
            "exchange_secret_encrypted",
        )?;
        report("recovery_rekey_offers");

        self.rekey_nullable_column(
            old_key,
            new_key,
            "recovery_rekey_answers",
            "answer_encrypted",
        )?;
        self.rekey_nullable_column(
            old_key,
            new_key,
            "recovery_rekey_answers",
            "old_shared_encrypted",
        )?;
        report("recovery_rekey_answers");

        self.rekey_nullable_column(old_key, new_key, "recovery_old_identity", "seed_encrypted")?;
        report("recovery_old_identity");

        self.rekey_nullable_column(old_key, new_key, "recovery_mode_key", "seed_encrypted")?;
        report("recovery_mode_key");

        self.rekey_nullable_column(
            old_key,
            new_key,
            "recovery_hand_backs",
            "delivery_encrypted",
        )?;
        report("recovery_hand_backs");

        self.rekey_nullable_column(
            old_key,
            new_key,
            "recovery_backup_pieces",
            "piece_encrypted",
        )?;
        report("recovery_backup_pieces");

        self.rekey_sync_timestamps(old_key, new_key)?;
        report("sync_timestamps");

        self.rekey_pending_updates(old_key, new_key)?;
        report("pending_updates");

        self.rekey_retry_entries(old_key, new_key)?;
        report("retry_entries");

        self.rekey_device_sync_checkpoints(old_key, new_key)?;
        report("device_sync_checkpoints");

        self.rekey_recovery_responses(old_key, new_key)?;
        report("recovery_responses");

        self.rekey_deletion_state(old_key, new_key)?;
        report("deletion_state");

        self.rekey_sync_checkpoints(old_key, new_key)?;
        report("sync_checkpoints");

        self.rekey_field_validations(old_key, new_key)?;
        report("field_validations");

        self.rekey_ux_state(old_key, new_key)?;
        self.rekey_user_presence_digest(old_key, new_key)?;
        report("ux_state");

        self.rekey_audit_log(old_key, new_key)?;
        report("audit_log");

        self.rekey_contacts_crypto(old_key, new_key)?;
        report("contacts_crypto");

        self.rekey_identity_passwords(old_key, new_key)?;
        report("identity_passwords");

        self.rekey_emergency_config(old_key, new_key)?;
        report("emergency_config");

        self.rekey_recovery_settings(old_key, new_key)?;
        report("recovery_settings");

        self.rekey_recovery_progress(old_key, new_key)?;
        report("recovery_progress");

        self.rekey_exchange_states(old_key, new_key)?;
        report("exchange_states");

        self.rekey_contact_display(old_key, new_key)?;
        report("contact_display");

        self.rekey_shared_avatars(old_key, new_key)?;
        report("shared_avatars");

        self.rekey_tags(old_key, new_key, "tags")?;
        report("tags");

        self.rekey_places(old_key, new_key, "places")?;
        report("places");

        self.rekey_exchange_location(old_key, new_key)?;
        self.rekey_nullable_column(
            old_key,
            new_key,
            "contacts",
            "last_sent_display_name_encrypted",
        )?;
        self.rekey_nullable_column(old_key, new_key, "contacts", "relay_url_encrypted")?;
        self.rekey_contact_vcard_uids(old_key, new_key)?;
        report("exchange_location");

        Ok(())
    }

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
        let old_mask_key = self.mask_key().clone();
        let new_mask_key = super::duress_content::mask_key(&new_key);
        const TOTAL_TABLES: u32 = 42;
        let mut completed: u32 = 0;

        let report = |completed: &mut u32, table: &str| {
            *completed += 1;
            if let Some(cb) = &progress {
                cb(*completed, TOTAL_TABLES, table);
            }
        };

        // The duress database begins first: if it cannot, no file
        // transaction is left open.
        let mask_conn = self.mask.as_deref().map(|mask| &mask.conn);
        if let Some(mask) = mask_conn {
            mask.execute_batch("BEGIN")?;
        }
        if let Err(e) = self.conn.execute_batch("BEGIN EXCLUSIVE TRANSACTION") {
            if let Some(mask) = mask_conn {
                mask.execute_batch("ROLLBACK")?;
            }
            return Err(e.into());
        }

        let result = (|| -> Result<(), StorageError> {
            self.rekey_tables(old_key, &new_key, &mut |table| {
                report(&mut completed, table)
            })?;
            self.rekey_mask(&old_mask_key, &new_mask_key)?;
            report(&mut completed, "mask_profile");

            super::key_check::rewrite(&self.conn, &new_key)?;
            match &self.mask {
                Some(mask) => super::mask::SealedSlot::of(&self.conn).write(
                    &super::duress_content::seal_body(&new_mask_key, &mask.sealed_body()?)?,
                ),
                None => Ok(()),
            }
        })()
        .and_then(|()| self.commit());

        match result {
            Ok(()) => {
                if let Some(mask) = self.mask.as_deref_mut() {
                    mask.conn.execute_batch("COMMIT")?;
                    mask.encryption_key = new_mask_key;
                }
                self.encryption_key = new_key;
                Ok(())
            }
            Err(e) => {
                // best-effort: we're already in the error path; if ROLLBACK
                // itself fails the transaction will be rolled back when the
                // connection drops anyway. `and` evaluates its argument
                // first, so the duress database rolls back too.
                #[allow(clippy::let_underscore_must_use)]
                let _ = self.conn.execute_batch("ROLLBACK").and(
                    self.mask
                        .as_deref()
                        .map_or(Ok(()), |mask| mask.conn.execute_batch("ROLLBACK")),
                );
                Err(e)
            }
        }
    }
}
