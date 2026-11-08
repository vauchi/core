// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Populate-every-encrypted-column rekey roundtrip.
//!
//! The existing `rekey_coverage_tests.rs` file has one test per
//! column. Each test populates exactly one column and leaves the rest
//! empty, so the per-row `if !enc.is_empty()` guards in `storage/rekey/` only
//! fire for that one branch — leaving most of the file unexercised.
//!
//! This test populates **every** column listed in `ENCRYPTED_COLUMNS`,
//! runs `Storage::rekey`, and verifies that every column round-trips
//! through the new key. A single test closes the long tail of
//! per-column re-encrypt branches.
//!
//! Special cases handled:
//! - `contact_ratchets.ratchet_state_encrypted` uses a per-contact
//!   HKDF-derived key (`vauchi-ratchet-storage-v1:<id>`).
//! - `visibility_labels.name_hmac` is recomputed during rekey using
//!   `Vauchi_Label_Name_HMAC_v1` derived from the new SEK.
//! - `recovery_settings.id`, `recovery_progress.id`, `own_card.id`,
//!   `device_registry.id`, `device_info.id`, `version_vector.id`,
//!   `ux_state.id`, `emergency_config.id`,
//!   `deletion_state.id` are singleton rows with `id = 1`.

use hmac::{Hmac, Mac};
use rusqlite::params;
use sha2::Sha256;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::crypto::kdf::HKDF;
use vauchi_core::crypto::{decrypt, encrypt};
use vauchi_core::storage::Storage;

mod fixtures;

use fixtures::{fixtures, fx};

type HmacSha256 = Hmac<Sha256>;

fn open_storage() -> (tempfile::TempDir, Storage, SymmetricKey) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let key = SymmetricKey::generate();
    let storage = Storage::open(&db_path, key.clone()).unwrap();
    (dir, storage, key)
}

fn ratchet_subkey(sek: &SymmetricKey, contact_id: &str) -> SymmetricKey {
    let mut info = b"vauchi-ratchet-storage-v1:".to_vec();
    info.extend_from_slice(contact_id.as_bytes());
    let derived = HKDF::derive_key(None, sek.as_bytes(), &info);
    SymmetricKey::from_bytes(*derived)
}

fn label_hmac(sek: &SymmetricKey, plain_name: &[u8]) -> Vec<u8> {
    let key_bytes = HKDF::derive_key(None, sek.as_bytes(), b"Vauchi_Label_Name_HMAC_v1");
    let mut mac = HmacSha256::new_from_slice(&*key_bytes).unwrap();
    mac.update(plain_name);
    mac.finalize().into_bytes().to_vec()
}

const CONTACT_ID: &str = "c1";
const CONTACT_PK: &[u8; 32] = b"\x01\x01\x01\x01\x01\x01\x01\x01\
                                \x01\x01\x01\x01\x01\x01\x01\x01\
                                \x01\x01\x01\x01\x01\x01\x01\x01\
                                \x01\x01\x01\x01\x01\x01\x01\x01";
const LABEL_ID: &str = "l1";

fn populate_every_column(storage: &Storage, key: &SymmetricKey) {
    let conn = storage.connection();
    let now = 1_700_000_000i64;

    // Helper closure to encrypt a fixture under the given key.
    let enc = |table: &str, column: &str| -> Vec<u8> { encrypt(key, &fx(table, column)).unwrap() };

    // ── contacts (also FK target for ratchets, sync_ts, field_notes,
    //     shared_avatars) ────────────────────────────────────────
    conn.execute(
        "INSERT INTO contacts \
         (id, public_key, card_encrypted, shared_key_encrypted, \
          personal_notes_encrypted, avatar_encrypted, \
          visibility_rules_encrypted, nickname_encrypted, custom_avatar_encrypted, \
          exchange_timestamp, contact_kind, exchange_location_encrypted, \
          last_sent_display_name_encrypted, relay_url_encrypted) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'exchanged', ?11, ?12, ?13)",
        params![
            CONTACT_ID,
            CONTACT_PK,
            enc("contacts", "card_encrypted"),
            enc("contacts", "shared_key_encrypted"),
            enc("contacts", "personal_notes_encrypted"),
            enc("contacts", "avatar_encrypted"),
            enc("contacts", "visibility_rules_encrypted"),
            enc("contacts", "nickname_encrypted"),
            enc("contacts", "custom_avatar_encrypted"),
            now,
            enc("contacts", "exchange_location_encrypted"),
            enc("contacts", "last_sent_display_name_encrypted"),
            enc("contacts", "relay_url_encrypted"),
        ],
    )
    .unwrap();

    // A second contact holds the CEK: its card is under that CEK, not the
    // storage key, as the app writes it (#585).
    let cek = vauchi_core::crypto::cek::ContentEncryptionKey::from_bytes(
        fx("contacts", "cek_encrypted").try_into().unwrap(),
    );
    conn.execute(
        "INSERT INTO contacts \
         (id, public_key, card_encrypted, shared_key_encrypted, cek_encrypted, \
          exchange_timestamp, contact_kind) \
         VALUES ('c2', ?1, ?2, ?3, ?4, ?5, 'exchanged')",
        params![
            [0x02u8; 32].as_slice(),
            cek.encrypt(&fx("contacts", "card_encrypted")).unwrap(),
            enc("contacts", "shared_key_encrypted"),
            enc("contacts", "cek_encrypted"),
            now,
        ],
    )
    .unwrap();

    // ── contact_device_registries (keyed by contact) ───────────
    conn.execute(
        "INSERT INTO contact_device_registries \
         (contact_id, broadcast_encrypted, version, updated_at) \
         VALUES (?1, ?2, 1, ?3)",
        params![
            CONTACT_ID,
            enc("contact_device_registries", "broadcast_encrypted"),
            now,
        ],
    )
    .unwrap();

    // ── safety_alert_facts (keyed by contact + nonce) ──────────
    conn.execute(
        "INSERT INTO safety_alert_facts \
         (contact_id, nonce, signed_payload_encrypted, received_at) \
         VALUES (?1, ?2, ?3, ?4)",
        params![
            CONTACT_ID,
            vec![0xA1u8; 32],
            enc("safety_alert_facts", "signed_payload_encrypted"),
            now,
        ],
    )
    .unwrap();

    // ── identity (singleton id=1) ──────────────────────────────
    conn.execute(
        "INSERT OR REPLACE INTO identity \
         (id, backup_data_encrypted, password_hash_encrypted, \
          duress_hash_encrypted, created_at) \
         VALUES (1, ?1, ?2, ?3, ?4)",
        params![
            enc("identity", "backup_data_encrypted"),
            enc("identity", "password_hash_encrypted"),
            enc("identity", "duress_hash_encrypted"),
            now,
        ],
    )
    .unwrap();

    // ── ratchet uses per-contact derived key ───────────────────
    let ratchet_key = ratchet_subkey(key, CONTACT_ID);
    let ratchet_blob = encrypt(
        &ratchet_key,
        &fx("contact_ratchets", "ratchet_state_encrypted"),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO contact_ratchets \
         (contact_id, ratchet_state_encrypted, is_initiator, updated_at) \
         VALUES (?1, ?2, 1, ?3)",
        params![CONTACT_ID, ratchet_blob, now],
    )
    .unwrap();

    // ── contact_field_notes ────────────────────────────────────
    conn.execute(
        "INSERT INTO contact_field_notes (contact_id, field_id, note_encrypted, updated_at) \
         VALUES (?1, 'email', ?2, ?3)",
        params![
            CONTACT_ID,
            enc("contact_field_notes", "note_encrypted"),
            now,
        ],
    )
    .unwrap();

    // ── own_card (singleton) ──────────────────────────────────
    // `card_json` is NOT NULL plaintext column kept for migration compat
    // (post-V14 it stays empty since real data lives in card_json_encrypted).
    conn.execute(
        "INSERT OR REPLACE INTO own_card (id, card_json, card_json_encrypted, updated_at) \
         VALUES (1, '', ?1, ?2)",
        params![enc("own_card", "card_json_encrypted"), now],
    )
    .unwrap();

    // ── device_registry (singleton) ───────────────────────────
    conn.execute(
        "INSERT OR REPLACE INTO device_registry \
         (id, registry_json, version, registry_json_encrypted, updated_at) \
         VALUES (1, '', 1, ?1, ?2)",
        params![enc("device_registry", "registry_json_encrypted"), now],
    )
    .unwrap();

    // ── device_sync_state (multi-row, keyed by device_id BLOB) ─
    let device_id_blob: Vec<u8> = vec![0xDEu8; 32];
    conn.execute(
        "INSERT INTO device_sync_state \
         (device_id, state_json, last_sync_version, state_json_encrypted, updated_at) \
         VALUES (?1, '', 0, ?2, ?3)",
        params![
            device_id_blob,
            enc("device_sync_state", "state_json_encrypted"),
            now,
        ],
    )
    .unwrap();

    // ── visibility_labels with name + hmac ─────────────────────
    let label_name_plain = fx("visibility_labels", "name_encrypted");
    conn.execute(
        "INSERT INTO visibility_labels \
         (id, contacts_json_encrypted, \
          visible_fields_json_encrypted, name_encrypted, name_hmac, \
          display_name_override_encrypted, created_at, modified_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            LABEL_ID,
            enc("visibility_labels", "contacts_json_encrypted"),
            enc("visibility_labels", "visible_fields_json_encrypted"),
            enc("visibility_labels", "name_encrypted"),
            label_hmac(key, &label_name_plain),
            enc("visibility_labels", "display_name_override_encrypted"),
            now,
            now,
        ],
    )
    .unwrap();

    // ── device_info (singleton) ───────────────────────────────
    conn.execute(
        "INSERT OR REPLACE INTO device_info (id, device_info_encrypted) VALUES (1, ?1)",
        params![enc("device_info", "device_info_encrypted")],
    )
    .unwrap();

    // ── version_vector (singleton) ────────────────────────────
    conn.execute(
        "INSERT OR REPLACE INTO version_vector \
         (id, vector_json_encrypted, updated_at) \
         VALUES (1, ?1, ?2)",
        params![enc("version_vector", "vector_json_encrypted"), now],
    )
    .unwrap();

    // ── receive_checkpoints (keyed by scope, #691) ────────────
    conn.execute(
        "INSERT INTO receive_checkpoints (scope, checkpoint_json_encrypted, updated_at) \
         VALUES ('scope-691', ?1, ?2)",
        params![enc("receive_checkpoints", "checkpoint_json_encrypted"), now],
    )
    .unwrap();

    // ── sync_field_timestamps (singleton) ─────────────────────
    conn.execute(
        "INSERT OR REPLACE INTO sync_field_timestamps \
         (id, timestamps_json_encrypted, updated_at) \
         VALUES (1, ?1, ?2)",
        params![
            enc("sync_field_timestamps", "timestamps_json_encrypted"),
            now
        ],
    )
    .unwrap();

    // ── contact_sync_timestamps ───────────────────────────────
    conn.execute(
        "INSERT INTO contact_sync_timestamps (contact_id, last_sync_at, last_sync_at_encrypted) \
         VALUES (?1, ?2, ?3)",
        params![
            CONTACT_ID,
            now,
            enc("contact_sync_timestamps", "last_sync_at_encrypted"),
        ],
    )
    .unwrap();

    // ── pending_updates ───────────────────────────────────────
    conn.execute(
        "INSERT INTO pending_updates \
         (id, contact_id, update_type, payload, payload_encrypted, created_at, retry_count, status) \
         VALUES ('upd1', ?1, 'card_delta', X'00', ?2, ?3, 0, 'pending')",
        params![
            CONTACT_ID,
            enc("pending_updates", "payload_encrypted"),
            now,
        ],
    )
    .unwrap();

    // ── retry_entries ─────────────────────────────────────────
    conn.execute(
        "INSERT INTO retry_entries \
         (message_id, recipient_id, payload, attempt, next_retry, created_at, payload_encrypted) \
         VALUES ('retry1', ?1, X'00', 0, ?2, ?3, ?4)",
        params![
            CONTACT_ID,
            now,
            now,
            enc("retry_entries", "payload_encrypted"),
        ],
    )
    .unwrap();

    // ── device_sync_checkpoints (target_device_id is BLOB) ────
    let target_id: Vec<u8> = vec![0xCAu8; 32];
    conn.execute(
        "INSERT INTO device_sync_checkpoints \
         (target_device_id, items_json, sent_count, items_json_encrypted, updated_at) \
         VALUES (?1, '', 0, ?2, ?3)",
        params![
            target_id,
            enc("device_sync_checkpoints", "items_json_encrypted"),
            now,
        ],
    )
    .unwrap();

    // ── recovery_responses ────────────────────────────────────
    conn.execute(
        "INSERT INTO recovery_responses \
         (claim_id, contact_id, response, response_encrypted, remind_at, created_at) \
         VALUES ('claim-1', ?1, '', ?2, NULL, ?3)",
        params![
            CONTACT_ID,
            enc("recovery_responses", "response_encrypted"),
            now,
        ],
    )
    .unwrap();

    // ── deletion_state (singleton) ────────────────────────────
    conn.execute(
        "INSERT OR REPLACE INTO deletion_state \
         (id, state_json, state_json_encrypted, updated_at) \
         VALUES (1, '', ?1, ?2)",
        params![enc("deletion_state", "state_json_encrypted"), now],
    )
    .unwrap();

    // ── sync_checkpoints ─────────────────────────────────────
    conn.execute(
        "INSERT INTO sync_checkpoints \
         (checkpoint_id, batch_id, total_items, processed_items, state_json, \
          state_json_encrypted, created_at, updated_at) \
         VALUES ('chk1', 'batch1', 1, 0, '', ?1, ?2, ?3)",
        params![enc("sync_checkpoints", "state_json_encrypted"), now, now],
    )
    .unwrap();

    // ── field_validations ────────────────────────────────────
    // field_value (TEXT) and signature (BLOB) are NOT NULL legacy columns.
    conn.execute(
        "INSERT INTO field_validations \
         (id, contact_id, field_id, field_value, validator_id, validated_at, signature, \
          field_value_encrypted, signature_encrypted) \
         VALUES ('fv1', ?1, 'email', '', 'validator-x', ?2, X'00', ?3, ?4)",
        params![
            CONTACT_ID,
            now,
            enc("field_validations", "field_value_encrypted"),
            enc("field_validations", "signature_encrypted"),
        ],
    )
    .unwrap();

    // ── ux_state (singleton) ─────────────────────────────────
    conn.execute(
        "INSERT OR REPLACE INTO ux_state \
         (id, \
          aha_tracker_json_encrypted, demo_contact_json_encrypted, \
          onboarding_progress_encrypted, backup_reminder_encrypted, updated_at, \
          settings_flags_encrypted, relay_url_encrypted, \
          own_card_repropagate_encrypted, exchange_defaults_encrypted, \
          relay_anchor_encrypted, user_presence_digest_encrypted) \
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            enc("ux_state", "aha_tracker_json_encrypted"),
            enc("ux_state", "demo_contact_json_encrypted"),
            enc("ux_state", "onboarding_progress_encrypted"),
            enc("ux_state", "backup_reminder_encrypted"),
            now,
            enc("ux_state", "settings_flags_encrypted"),
            enc("ux_state", "relay_url_encrypted"),
            enc("ux_state", "own_card_repropagate_encrypted"),
            enc("ux_state", "exchange_defaults_encrypted"),
            enc("ux_state", "relay_anchor_encrypted"),
            enc("ux_state", "user_presence_digest_encrypted"),
        ],
    )
    .unwrap();

    // ── tags and places (keyed by id) ────────────────────────
    conn.execute(
        "INSERT INTO tags (id, name_encrypted, created_at) VALUES ('t1', ?1, ?2)",
        params![enc("tags", "name_encrypted"), now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO places (id, data_encrypted, created_at) VALUES ('p1', ?1, ?2)",
        params![enc("places", "data_encrypted"), now],
    )
    .unwrap();

    // ── the mask profile (ADR-080): a regular table and the two only it
    // has, under the mask key ───────
    let mask = crate::common::helpers::duress_mask_key(key);
    let enc =
        |table: &str, column: &str| -> Vec<u8> { encrypt(&mask, &fx(table, column)).unwrap() };
    let duress = storage.duress_connection();
    duress
        .execute(
            "INSERT INTO tags (id, name_encrypted, created_at) VALUES ('mt1', ?1, ?2)",
            params![enc("tags", "name_encrypted"), now],
        )
        .unwrap();
    duress
        .execute(
            "INSERT OR REPLACE INTO mask_seed (id, seed_encrypted) VALUES (1, ?1)",
            params![enc("mask_seed", "seed_encrypted")],
        )
        .unwrap();
    duress
        .execute(
            "INSERT INTO decoy_ids \
             (contact_id, decoy_id, display_name_encrypted, created_at, updated_at) \
             VALUES ('c-d1', 'd1', ?1, ?2, ?2)",
            params![enc("decoy_ids", "display_name_encrypted"), now],
        )
        .unwrap();

    let enc = |table: &str, column: &str| -> Vec<u8> { encrypt(key, &fx(table, column)).unwrap() };

    // ── audit_log ────────────────────────────────────────────
    conn.execute(
        "INSERT INTO audit_log (event_type, details, details_encrypted, timestamp) \
         VALUES ('test', NULL, ?1, ?2)",
        params![enc("audit_log", "details_encrypted"), now],
    )
    .unwrap();

    // ── emergency_config (singleton) ─────────────────────────
    conn.execute(
        "INSERT INTO emergency_config \
         (id, trusted_contact_ids_encrypted, message_encrypted, created_at, updated_at) \
         VALUES (1, ?1, ?2, ?3, ?4)",
        params![
            enc("emergency_config", "trusted_contact_ids_encrypted"),
            enc("emergency_config", "message_encrypted"),
            now,
            now,
        ],
    )
    .unwrap();

    // ── recovery_settings (singleton) ────────────────────────
    conn.execute(
        "INSERT INTO recovery_settings (id, settings_encrypted, updated_at) \
         VALUES (1, ?1, ?2)",
        params![enc("recovery_settings", "settings_encrypted"), now],
    )
    .unwrap();

    // ── exchange_states ──────────────────────────────────────
    conn.execute(
        "INSERT INTO exchange_states \
         (exchange_id, encrypted_blob, created_at, expires_at) \
         VALUES ('ex1', ?1, ?2, ?3)",
        params![enc("exchange_states", "encrypted_blob"), now, now + 300],
    )
    .unwrap();

    // ── contact_shared_avatars ───────────────────────────────
    conn.execute(
        "INSERT INTO contact_shared_avatars \
         (contact_id, avatar_hash, avatar_encrypted, is_primary, updated_at) \
         VALUES (?1, 'hash-1', ?2, 1, ?3)",
        params![
            CONTACT_ID,
            enc("contact_shared_avatars", "avatar_encrypted"),
            now,
        ],
    )
    .unwrap();

    // ── recovery_progress (singleton) ────────────────────────
    conn.execute(
        "INSERT INTO recovery_progress (id, progress_encrypted, updated_at) \
         VALUES (1, ?1, ?2)",
        params![enc("recovery_progress", "progress_encrypted"), now],
    )
    .unwrap();
}

#[allow(clippy::too_many_lines)]
/// The tables only the mask profile has (ADR-080).
const MASK_ONLY_TABLES: [&str; 2] = ["mask_seed", "decoy_ids"];

fn assert_every_column_round_trips(storage: &Storage, new_key: &SymmetricKey) {
    let conn = storage.connection();
    let conn_for = |table: &str| {
        if MASK_ONLY_TABLES.contains(&table) {
            storage.duress_connection()
        } else {
            conn
        }
    };

    let new_mask_key = crate::common::helpers::duress_mask_key(new_key);
    let check_one =
        |table: &str, column: &str, where_clause: &str, params_: &[&dyn rusqlite::ToSql]| {
            let sql = format!("SELECT {} FROM {} WHERE {}", column, table, where_clause);
            let blob: Vec<u8> = conn_for(table)
                .query_row(&sql, params_, |row| row.get(0))
                .unwrap_or_else(|e| panic!("read {}.{}: {}", table, column, e));
            let table_key = if MASK_ONLY_TABLES.contains(&table) {
                &new_mask_key
            } else {
                new_key
            };
            let plain = decrypt(table_key, &blob)
                .unwrap_or_else(|e| panic!("decrypt {}.{} with new key: {}", table, column, e));
            assert_eq!(plain, fx(table, column), "{}.{} plaintext", table, column);
        };

    let id_eq_1: &[&dyn rusqlite::ToSql] = &[];
    let by_contact: &[&dyn rusqlite::ToSql] = &[&CONTACT_ID];

    // ── contacts (10 columns on c1, the CEK on c2) ────────────────────
    for col in [
        "card_encrypted",
        "shared_key_encrypted",
        "personal_notes_encrypted",
        "avatar_encrypted",
        "visibility_rules_encrypted",
        "nickname_encrypted",
        "custom_avatar_encrypted",
        "exchange_location_encrypted",
        "last_sent_display_name_encrypted",
        "relay_url_encrypted",
    ] {
        check_one("contacts", col, "id = ?1", by_contact);
    }
    check_one("contacts", "cek_encrypted", "id = 'c2'", id_eq_1);

    // ── tags and places (keyed by id) ────────────────────────
    check_one("tags", "name_encrypted", "id = 't1'", id_eq_1);
    check_one("places", "data_encrypted", "id = 'p1'", id_eq_1);

    // ── the mask profile: its regular tables under the mask key ───
    let mask_tag: Vec<u8> = storage
        .duress_connection()
        .query_row(
            "SELECT name_encrypted FROM tags WHERE id = 'mt1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        decrypt(&new_mask_key, &mask_tag).unwrap(),
        fx("tags", "name_encrypted"),
        "the mask profile's tag is under the new mask key"
    );
    check_one("mask_seed", "seed_encrypted", "id = 1", id_eq_1);
    check_one(
        "decoy_ids",
        "display_name_encrypted",
        "contact_id = 'c-d1'",
        id_eq_1,
    );

    // ── identity (3 columns, singleton) ─────────────────────
    for col in [
        "backup_data_encrypted",
        "password_hash_encrypted",
        "duress_hash_encrypted",
    ] {
        check_one("identity", col, "id = 1", id_eq_1);
    }

    // ── ratchet: per-contact derived key ────────────────────
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT ratchet_state_encrypted FROM contact_ratchets WHERE contact_id = ?1",
            [&CONTACT_ID],
            |row| row.get(0),
        )
        .unwrap();
    let ratchet_key = ratchet_subkey(new_key, CONTACT_ID);
    let plain = decrypt(&ratchet_key, &blob).unwrap();
    assert_eq!(
        plain,
        fx("contact_ratchets", "ratchet_state_encrypted"),
        "ratchet plaintext through new per-contact key"
    );

    // ── contact_field_notes ─────────────────────────────────
    check_one(
        "contact_field_notes",
        "note_encrypted",
        "contact_id = ?1 AND field_id = 'email'",
        by_contact,
    );

    // ── contact_device_registries ───────────────────────────
    check_one(
        "contact_device_registries",
        "broadcast_encrypted",
        "contact_id = ?1",
        by_contact,
    );

    // ── safety_alert_facts ──────────────────────────────────
    check_one(
        "safety_alert_facts",
        "signed_payload_encrypted",
        "contact_id = ?1",
        by_contact,
    );

    // ── singleton tables (id = 1) ───────────────────────────
    for (table, col) in [
        ("own_card", "card_json_encrypted"),
        ("device_registry", "registry_json_encrypted"),
        ("device_info", "device_info_encrypted"),
        ("version_vector", "vector_json_encrypted"),
        ("sync_field_timestamps", "timestamps_json_encrypted"),
        ("deletion_state", "state_json_encrypted"),
        ("emergency_config", "trusted_contact_ids_encrypted"),
        ("emergency_config", "message_encrypted"),
        ("recovery_settings", "settings_encrypted"),
        ("recovery_progress", "progress_encrypted"),
        ("ux_state", "aha_tracker_json_encrypted"),
        ("ux_state", "demo_contact_json_encrypted"),
        ("ux_state", "onboarding_progress_encrypted"),
        ("ux_state", "backup_reminder_encrypted"),
        ("ux_state", "settings_flags_encrypted"),
        ("ux_state", "relay_url_encrypted"),
        ("ux_state", "own_card_repropagate_encrypted"),
        ("ux_state", "exchange_defaults_encrypted"),
        ("ux_state", "relay_anchor_encrypted"),
        ("ux_state", "user_presence_digest_encrypted"),
    ] {
        check_one(table, col, "id = 1", id_eq_1);
    }

    // ── device_sync_state ───────────────────────────────────
    let device_id_blob: Vec<u8> = vec![0xDEu8; 32];
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT state_json_encrypted FROM device_sync_state WHERE device_id = ?1",
            [&device_id_blob],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        decrypt(new_key, &blob).unwrap(),
        fx("device_sync_state", "state_json_encrypted")
    );

    // ── visibility_labels ───────────────────────────────────
    let (contacts_b, fields_b, name_b, hmac_b, override_b): (
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
    ) = conn
        .query_row(
            "SELECT contacts_json_encrypted, visible_fields_json_encrypted, \
                    name_encrypted, name_hmac, display_name_override_encrypted \
             FROM visibility_labels WHERE id = ?1",
            [&LABEL_ID],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(
        decrypt(new_key, &contacts_b).unwrap(),
        fx("visibility_labels", "contacts_json_encrypted")
    );
    assert_eq!(
        decrypt(new_key, &fields_b).unwrap(),
        fx("visibility_labels", "visible_fields_json_encrypted")
    );
    assert_eq!(
        decrypt(new_key, &name_b).unwrap(),
        fx("visibility_labels", "name_encrypted")
    );
    assert_eq!(
        decrypt(new_key, &override_b).unwrap(),
        fx("visibility_labels", "display_name_override_encrypted")
    );
    assert_eq!(
        hmac_b,
        label_hmac(new_key, &fx("visibility_labels", "name_encrypted")),
        "name_hmac must be recomputed with the new SEK's HMAC subkey"
    );

    // ── contact_sync_timestamps ─────────────────────────────
    check_one(
        "contact_sync_timestamps",
        "last_sync_at_encrypted",
        "contact_id = ?1",
        by_contact,
    );

    // ── pending_updates / retry_entries ─────────────────────
    check_one("pending_updates", "payload_encrypted", "id = 'upd1'", &[]);
    check_one(
        "retry_entries",
        "payload_encrypted",
        "message_id = 'retry1'",
        &[],
    );

    // ── device_sync_checkpoints ─────────────────────────────
    let target_id: Vec<u8> = vec![0xCAu8; 32];
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT items_json_encrypted FROM device_sync_checkpoints WHERE target_device_id = ?1",
            [&target_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        decrypt(new_key, &blob).unwrap(),
        fx("device_sync_checkpoints", "items_json_encrypted")
    );

    // ── recovery_responses ──────────────────────────────────
    check_one(
        "recovery_responses",
        "response_encrypted",
        "claim_id = 'claim-1'",
        &[],
    );

    // ── sync_checkpoints ────────────────────────────────────
    check_one(
        "sync_checkpoints",
        "state_json_encrypted",
        "checkpoint_id = 'chk1'",
        &[],
    );

    // ── field_validations ───────────────────────────────────
    check_one(
        "field_validations",
        "field_value_encrypted",
        "id = 'fv1'",
        &[],
    );
    check_one(
        "field_validations",
        "signature_encrypted",
        "id = 'fv1'",
        &[],
    );

    // ── audit_log (id is auto-increment, fetch the only row) ─
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT details_encrypted FROM audit_log LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        decrypt(new_key, &blob).unwrap(),
        fx("audit_log", "details_encrypted")
    );

    // ── exchange_states ─────────────────────────────────────
    check_one(
        "exchange_states",
        "encrypted_blob",
        "exchange_id = 'ex1'",
        &[],
    );

    // ── contact_shared_avatars ──────────────────────────────
    check_one(
        "contact_shared_avatars",
        "avatar_encrypted",
        "contact_id = ?1 AND avatar_hash = 'hash-1'",
        by_contact,
    );
}

// ============================================================
// ============================================================

// @scenario: security :: rekey re-encrypts every column in the registry
// @internal
#[test]
fn rekey_round_trips_every_encrypted_column_in_one_pass() {
    let (_dir, mut storage, key1) = open_storage();

    populate_every_column(&storage, &key1);

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    assert_every_column_round_trips(&storage, &key2);
}

/// Regression: a second rekey on the already-rekeyed database must
/// also succeed and round-trip every column.
// @internal
#[test]
fn rekey_is_idempotent_under_repeated_calls() {
    let (_dir, mut storage, key1) = open_storage();
    populate_every_column(&storage, &key1);

    let key2 = SymmetricKey::generate();
    let key3 = SymmetricKey::generate();
    storage.rekey(key2).unwrap();
    storage.rekey(key3.clone()).unwrap();

    assert_every_column_round_trips(&storage, &key3);
}

/// Columns this file does not populate, each covered elsewhere. A column added
/// to `ENCRYPTED_COLUMNS` must join the fixtures above or this list, so the
/// "every column" claim cannot silently go stale again (it had, for seven
/// columns, until vauchi/private#522).
const COVERED_ELSEWHERE: &[(&str, &str)] = &[
    // peer_card_ordering_survives_storage_key_rotation in sync_store_tests.rs.
    ("peer_card_ordering", "state_encrypted"),
    // the_key_survives_a_storage_rekey in device_signing_key_tests.rs.
    ("device_signing_key", "key_encrypted"),
    // the_revoked_contacts_name_survives_a_key_rotation in
    // recovery_after_revocation_tests.rs.
    ("revoked_senders", "display_name_encrypted"),
    // a_kept_backup_and_waiting_pieces_survive_a_storage_rekey in
    // guardian_backup_keep_tests.rs.
    ("guardian_backup_pieces", "piece_encrypted"),
    ("guardian_backups", "delivery_encrypted"),
    // a_returned_backup_survives_a_storage_rekey in
    // guardian_backup_return_tests.rs.
    ("returned_backup_pieces", "piece_encrypted"),
    ("returned_backups", "delivery_encrypted"),
    // The social recovery tables (#796), each with a survives_a_storage_rekey
    // test in guardian_backup_passphrase_tests.rs and recovery_rekey_*_tests.rs.
    ("recovery_passphrase", "key_encrypted"),
    ("recovery_own_proof", "proof_encrypted"),
    ("recovery_rekey_offers", "exchange_secret_encrypted"),
    ("recovery_rekey_answers", "answer_encrypted"),
    ("recovery_rekey_answers", "old_shared_encrypted"),
    ("recovery_old_identity", "seed_encrypted"),
    ("recovery_mode_key", "seed_encrypted"),
    ("recovery_hand_backs", "delivery_encrypted"),
    ("recovery_backup_pieces", "piece_encrypted"),
    // the_vcard_uid_lookup_survives_a_rekey in
    // identifying_columns_at_rest_tests.rs (value and lookup hash).
    ("contacts", "original_uid_encrypted"),
    ("contacts", "original_uid_hmac"),
    // test_rekey_preserves_label_bio_and_avatar_overrides in
    // rekey_coverage_tests.rs.
    ("visibility_labels", "avatar_override_encrypted"),
    ("visibility_labels", "bio_override_encrypted"),
    // Recomputed (not re-encrypted) during rekey; populated and checked in
    // this file through label_hmac rather than through the fixtures.
    ("visibility_labels", "name_hmac"),
    // Re-sealed whole from the duress tables above;
    // the_duress_content_survives_a_storage_rekey in duress_at_rest_tests.rs.
    ("duress_content", "content_encrypted"),
];

// @internal
#[test]
fn the_fixtures_cover_every_registered_encrypted_column() {
    let fixtures: Vec<(&str, &str)> = fixtures().iter().map(|(t, c, _)| (*t, *c)).collect();

    let missing: Vec<&(&str, &str)> = vauchi_core::storage::ENCRYPTED_COLUMNS
        .iter()
        .filter(|column| !fixtures.contains(column) && !COVERED_ELSEWHERE.contains(column))
        .collect();

    assert!(missing.is_empty(), "{missing:?}");
}
