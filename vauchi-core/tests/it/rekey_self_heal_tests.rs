// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Self-heal branch coverage for `storage/rekey.rs::rekey_or_heal`.
//!
//! V4 / V32 migrations added `_encrypted` columns but a known
//! pre-encryption gap meant some callers wrote plaintext to those
//! columns. Rekey detects this by trying to decrypt with the old key:
//! valid ciphertexts start with algorithm tag 0x02 / 0x03, and UTF-8
//! text never starts with those bytes, so a decrypt failure means the
//! data is plaintext. The self-heal path encrypts the plaintext with
//! the new key, fixing the gap in place.
//!
//! These tests populate `personal_notes_encrypted`, `avatar_encrypted`,
//! and `note_encrypted` with **raw plaintext** (bypassing
//! `crate::crypto::encrypt`), run rekey, and verify each column now
//! decrypts cleanly with the new key.

use rusqlite::params;
use vauchi_core::crypto::{SymmetricKey, decrypt};
use vauchi_core::storage::Storage;

const CONTACT_ID: &str = "c1";
const CONTACT_PK: &[u8; 32] = &[0x42u8; 32];

fn open_storage_with_contact() -> (tempfile::TempDir, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();

    // Insert the parent contact row so FK-constrained tables accept inserts.
    let key1 = storage.key().clone();
    let card_enc = vauchi_core::crypto::encrypt(&key1, b"{\"name\":\"Bob\"}").unwrap();
    let sk_enc = vauchi_core::crypto::encrypt(&key1, b"shared-key-bytes-32-padding!!!ab").unwrap();
    storage
        .connection()
        .execute(
            "INSERT INTO contacts \
             (id, public_key, display_name, card_encrypted, shared_key_encrypted, \
              exchange_timestamp, contact_kind) \
             VALUES (?1, ?2, 'Bob', ?3, ?4, 1000, 'exchanged')",
            params![CONTACT_ID, CONTACT_PK, card_enc, sk_enc],
        )
        .unwrap();

    (dir, storage)
}

// @scenario: security :: rekey self-heals plaintext personal_notes
// @internal
#[test]
fn rekey_heals_plaintext_personal_notes_encrypted_column() {
    let (_dir, mut storage) = open_storage_with_contact();
    let plaintext = b"loves dogs";
    storage
        .connection()
        .execute(
            "UPDATE contacts SET personal_notes_encrypted = ?1 WHERE id = ?2",
            params![plaintext, CONTACT_ID],
        )
        .unwrap();

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    let blob: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT personal_notes_encrypted FROM contacts WHERE id = ?1",
            [&CONTACT_ID],
            |row| row.get(0),
        )
        .unwrap();
    let decrypted = decrypt(&key2, &blob).expect("rekey must heal plaintext to ciphertext");
    assert_eq!(decrypted, plaintext);
}

// @scenario: security :: rekey self-heals plaintext avatar
// @internal
#[test]
fn rekey_heals_plaintext_avatar_encrypted_column() {
    let (_dir, mut storage) = open_storage_with_contact();
    let plaintext = b"AVATARBYTES";
    storage
        .connection()
        .execute(
            "UPDATE contacts SET avatar_encrypted = ?1 WHERE id = ?2",
            params![plaintext, CONTACT_ID],
        )
        .unwrap();

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    let blob: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT avatar_encrypted FROM contacts WHERE id = ?1",
            [&CONTACT_ID],
            |row| row.get(0),
        )
        .unwrap();
    let decrypted = decrypt(&key2, &blob).expect("rekey must heal plaintext avatar");
    assert_eq!(decrypted, plaintext);
}

// @scenario: security :: rekey self-heals plaintext field_note
// @internal
#[test]
fn rekey_heals_plaintext_field_note() {
    let (_dir, mut storage) = open_storage_with_contact();
    let plaintext = b"this phone is work";
    storage
        .connection()
        .execute(
            "INSERT INTO contact_field_notes (contact_id, field_id, note_encrypted, updated_at) \
             VALUES (?1, 'email', ?2, 1000)",
            params![CONTACT_ID, plaintext],
        )
        .unwrap();

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    let blob: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT note_encrypted FROM contact_field_notes \
             WHERE contact_id = ?1 AND field_id = 'email'",
            [&CONTACT_ID],
            |row| row.get(0),
        )
        .unwrap();
    let decrypted = decrypt(&key2, &blob).expect("rekey must heal plaintext field_note");
    assert_eq!(decrypted, plaintext);
}

// @scenario: security :: rekey heals one row, normally-encrypts another
// @internal
#[test]
fn rekey_handles_mixed_plaintext_and_ciphertext_in_same_column() {
    // Two contacts, two states: one healed, one regular rekey.
    let (_dir, mut storage) = open_storage_with_contact();
    let key1 = storage.key().clone();

    // Add second contact.
    let other_id = "c2";
    let other_pk = [0x99u8; 32];
    let card_enc = vauchi_core::crypto::encrypt(&key1, b"{\"name\":\"Carol\"}").unwrap();
    let sk_enc = vauchi_core::crypto::encrypt(&key1, b"shared-key-bytes-32-padding!!!cc").unwrap();
    storage
        .connection()
        .execute(
            "INSERT INTO contacts \
             (id, public_key, display_name, card_encrypted, shared_key_encrypted, \
              exchange_timestamp, contact_kind) \
             VALUES (?1, ?2, 'Carol', ?3, ?4, 1000, 'exchanged')",
            params![other_id, &other_pk[..], card_enc, sk_enc],
        )
        .unwrap();

    // c1 has plaintext notes (the gap). c2 has properly-encrypted notes.
    let c1_plain = b"notes-gap";
    let c2_plain = b"notes-encrypted";
    let c2_enc = vauchi_core::crypto::encrypt(&key1, c2_plain).unwrap();
    storage
        .connection()
        .execute(
            "UPDATE contacts SET personal_notes_encrypted = ?1 WHERE id = ?2",
            params![c1_plain, CONTACT_ID],
        )
        .unwrap();
    storage
        .connection()
        .execute(
            "UPDATE contacts SET personal_notes_encrypted = ?1 WHERE id = ?2",
            params![c2_enc, other_id],
        )
        .unwrap();

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    let c1_blob: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT personal_notes_encrypted FROM contacts WHERE id = ?1",
            [&CONTACT_ID],
            |row| row.get(0),
        )
        .unwrap();
    let c2_blob: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT personal_notes_encrypted FROM contacts WHERE id = ?1",
            [&other_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        decrypt(&key2, &c1_blob).expect("c1 healed"),
        c1_plain,
        "row written as plaintext is healed"
    );
    assert_eq!(
        decrypt(&key2, &c2_blob).expect("c2 normal rekey"),
        c2_plain,
        "row already encrypted with old key is re-encrypted with new key"
    );
}

// @scenario: security :: rekey-or-heal preserves byte-exact plaintext that
//                       happens to contain non-UTF8 bytes
// @internal
#[test]
fn rekey_heals_plaintext_with_arbitrary_byte_content() {
    let (_dir, mut storage) = open_storage_with_contact();
    // Bytes that are not a valid ciphertext header AND not valid UTF-8 either —
    // tests that the self-heal path doesn't assume UTF-8.
    // Avoid leading 0x02 / 0x03 (algorithm tags) so decrypt fails reliably.
    let plaintext = vec![0xFFu8, 0xFE, 0xFD, 0x00, 0x80, 0x81, 0x42];
    storage
        .connection()
        .execute(
            "UPDATE contacts SET personal_notes_encrypted = ?1 WHERE id = ?2",
            params![plaintext.clone(), CONTACT_ID],
        )
        .unwrap();

    let key2 = SymmetricKey::generate();
    storage.rekey(key2.clone()).unwrap();

    let blob: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT personal_notes_encrypted FROM contacts WHERE id = ?1",
            [&CONTACT_ID],
            |row| row.get(0),
        )
        .unwrap();
    let decrypted = decrypt(&key2, &blob).expect("rekey must heal arbitrary-bytes plaintext");
    assert_eq!(decrypted, plaintext);
}
