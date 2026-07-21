// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Durable received safety-alert facts (`safety_alert_facts`, migration v62).
//!
//! A verified duress/emergency alert must survive any crash between
//! receive-commit and surfacing: the receive path burns the replay nonce, so
//! an alert that exists only in memory is unrecoverable after a crash
//! (delivery-axis findings, `2026-07-21-per-device-ratchet-registry-dormant`).

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::*;

fn create_test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

fn open_file_storage() -> (tempfile::TempDir, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();
    (dir, storage)
}

fn saved_contact(storage: &Storage, name: &str) -> String {
    let public_key = [0u8; 32];
    let mut card = ContactCard::new(name);
    let _ = card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        &format!("{}@example.com", name.to_lowercase()),
        0,
    ));
    let contact = Contact::from_exchange(public_key, card, SymmetricKey::generate(), 0);
    let id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();
    id
}

// @internal
#[test]
fn insert_fact_if_absent_inserts_then_ignores_and_is_immutable() {
    let storage = create_test_storage();
    let contact_id = saved_contact(&storage, "Alice");
    let nonce = [7u8; 32];

    let inserted = storage
        .safety_alerts()
        .insert_fact_if_absent(&contact_id, &nonce, b"signed-alert-original", 1111)
        .unwrap();
    assert!(inserted, "first insert must report insertion");

    let second = storage
        .safety_alerts()
        .insert_fact_if_absent(&contact_id, &nonce, b"signed-alert-TAMPERED", 2222)
        .unwrap();
    assert!(
        !second,
        "same (contact, nonce) must be ignored, not replaced"
    );

    let facts = storage.safety_alerts().load_unsurfaced_facts().unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].contact_id, contact_id);
    assert_eq!(facts[0].nonce, nonce);
    assert_eq!(
        facts[0].signed_payload, b"signed-alert-original",
        "first fact is immutable — a second payload must never overwrite it"
    );
    assert_eq!(facts[0].received_at, 1111);
}

// @internal
#[test]
fn insert_or_compare_fact_inserts_dedups_and_rejects_mismatch() {
    // F9: genesis receive needs integrity on conflict, not silent idempotency —
    // a second alert under the same (contact, nonce) with DIFFERENT signed
    // bytes is a collision/tamper and must fail closed, while an identical
    // re-delivery is a benign duplicate.
    use vauchi_core::storage::GenesisFactWrite;

    let storage = create_test_storage();
    let contact_id = saved_contact(&storage, "Alice");
    let nonce = [11u8; 32];

    let first = storage
        .safety_alerts()
        .insert_or_compare_fact(&contact_id, &nonce, b"signed-alert", 100)
        .unwrap();
    assert_eq!(first, GenesisFactWrite::Inserted);

    let dup = storage
        .safety_alerts()
        .insert_or_compare_fact(&contact_id, &nonce, b"signed-alert", 200)
        .unwrap();
    assert_eq!(
        dup,
        GenesisFactWrite::Duplicate,
        "identical re-delivery must be a benign duplicate"
    );

    let mismatch = storage.safety_alerts().insert_or_compare_fact(
        &contact_id,
        &nonce,
        b"different-bytes",
        300,
    );
    assert!(
        mismatch.is_err(),
        "same nonce with different signed bytes must fail closed, not silently dedup"
    );

    // The original fact is untouched by the rejected mismatch.
    let facts = storage.safety_alerts().load_unsurfaced_facts().unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].signed_payload, b"signed-alert");
    assert_eq!(facts[0].received_at, 100);
}

// @internal
#[test]
fn load_unsurfaced_facts_excludes_surfaced() {
    let storage = create_test_storage();
    let contact_id = saved_contact(&storage, "Bob");
    let nonce_a = [1u8; 32];
    let nonce_b = [2u8; 32];

    storage
        .safety_alerts()
        .insert_fact_if_absent(&contact_id, &nonce_a, b"alert-a", 10)
        .unwrap();
    storage
        .safety_alerts()
        .insert_fact_if_absent(&contact_id, &nonce_b, b"alert-b", 20)
        .unwrap();

    storage
        .safety_alerts()
        .mark_fact_surfaced(&contact_id, &nonce_a, 30)
        .unwrap();

    let facts = storage.safety_alerts().load_unsurfaced_facts().unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].nonce, nonce_b);
    assert_eq!(facts[0].signed_payload, b"alert-b");
    assert_eq!(facts[0].received_at, 20);
}

// @internal
#[test]
fn mark_fact_surfaced_is_idempotent() {
    let storage = create_test_storage();
    let contact_id = saved_contact(&storage, "Carol");
    let nonce = [3u8; 32];

    storage
        .safety_alerts()
        .insert_fact_if_absent(&contact_id, &nonce, b"alert-c", 40)
        .unwrap();
    storage
        .safety_alerts()
        .mark_fact_surfaced(&contact_id, &nonce, 41)
        .unwrap();
    storage
        .safety_alerts()
        .mark_fact_surfaced(&contact_id, &nonce, 42)
        .unwrap();

    assert!(
        storage
            .safety_alerts()
            .load_unsurfaced_facts()
            .unwrap()
            .is_empty()
    );
}

// @scenario: security :: Sensitive data is encrypted at rest (ADR-015)
#[test]
fn alert_fact_payload_encrypted_at_rest() {
    let (_dir, storage) = open_file_storage();
    let contact_id = saved_contact(&storage, "Dave");
    let nonce = [4u8; 32];
    let needle: &[u8] = b"COERCION-ALERT-PLAINTEXT-MARKER";

    storage
        .safety_alerts()
        .insert_fact_if_absent(&contact_id, &nonce, needle, 50)
        .unwrap();

    let raw: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT signed_payload_encrypted FROM safety_alert_facts WHERE contact_id = ?1",
            rusqlite::params![contact_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !raw.windows(needle.len()).any(|w| w == needle),
        "alert plaintext must not appear in the stored BLOB"
    );
}

// @internal
#[test]
fn contact_delete_cascades_alert_facts() {
    let storage = create_test_storage();
    let contact_id = saved_contact(&storage, "Eve");
    let nonce = [5u8; 32];

    storage
        .safety_alerts()
        .insert_fact_if_absent(&contact_id, &nonce, b"alert-e", 60)
        .unwrap();
    assert!(storage.delete_contact(&contact_id).unwrap());

    assert!(
        storage
            .safety_alerts()
            .load_unsurfaced_facts()
            .unwrap()
            .is_empty(),
        "deleting the contact must delete its alert facts (FK cascade)"
    );
}

// @internal
#[test]
fn migration_v62_creates_safety_alert_facts_table() {
    let storage = create_test_storage();
    let mut columns: Vec<String> = Vec::new();
    storage
        .connection()
        .prepare("PRAGMA table_info(safety_alert_facts)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .for_each(|c| columns.push(c.unwrap()));

    for expected in [
        "contact_id",
        "nonce",
        "signed_payload_encrypted",
        "received_at",
        "fanout_queued_at",
        "surfaced_at",
    ] {
        assert!(
            columns.iter().any(|c| c == expected),
            "safety_alert_facts missing column: {expected}"
        );
    }
}

// @internal
#[test]
fn corrupt_fact_row_does_not_suppress_healthy_facts() {
    let storage = create_test_storage();
    let contact_id = saved_contact(&storage, "Frank");

    storage
        .safety_alerts()
        .insert_fact_if_absent(&contact_id, &[6u8; 32], b"healthy-alert", 70)
        .unwrap();
    // A row whose payload was corrupted on disk (not decryptable with the
    // storage key) must not abort the load — one bad row must never hide a
    // healthy life-safety alert.
    storage
        .connection()
        .execute(
            "INSERT INTO safety_alert_facts
                 (contact_id, nonce, signed_payload_encrypted, received_at)
                 VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                contact_id,
                [8u8; 32],
                b"garbage-not-ciphertext".to_vec(),
                60
            ],
        )
        .unwrap();

    let facts = storage.safety_alerts().load_unsurfaced_facts().unwrap();
    assert_eq!(
        facts.len(),
        1,
        "healthy fact must survive a corrupt sibling"
    );
    assert_eq!(facts[0].nonce, [6u8; 32]);
    assert_eq!(facts[0].signed_payload, b"healthy-alert");
}
