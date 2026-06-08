// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for StorageError disk-full detection and user messages.

use vauchi_core::storage::StorageError;

// @internal
#[test]
fn sqlite_full_error_code_13_converts_to_disk_full() {
    // SQLITE_FULL has extended_code = 13
    let ffi_err = rusqlite::ffi::Error::new(13);
    let sqlite_err = rusqlite::Error::SqliteFailure(ffi_err, Some("database is full".into()));
    let storage_err: StorageError = sqlite_err.into();
    assert!(
        matches!(storage_err, StorageError::DiskFull),
        "SQLITE_FULL (code 13) should map to DiskFull, got: {storage_err:?}"
    );
}

// @internal
#[test]
fn other_sqlite_errors_remain_database() {
    // SQLITE_BUSY has extended_code = 5
    let ffi_err = rusqlite::ffi::Error::new(5);
    let sqlite_err = rusqlite::Error::SqliteFailure(ffi_err, Some("database is locked".into()));
    let storage_err: StorageError = sqlite_err.into();
    assert!(
        matches!(storage_err, StorageError::Database(_)),
        "SQLITE_BUSY should remain Database, got: {storage_err:?}"
    );
}

// @internal
#[test]
fn disk_full_user_message_is_actionable() {
    let err = StorageError::DiskFull;
    let msg = err.user_message();
    assert!(msg.contains("storage is full"), "Message: {msg}");
    assert!(msg.contains("Free up space"), "Message: {msg}");
}

// @internal
#[test]
fn queue_full_user_message_mentions_sync() {
    let err = StorageError::QueueFull("test".into());
    let msg = err.user_message();
    assert!(msg.contains("sync"), "Message: {msg}");
}

// @internal
#[test]
fn generic_error_user_message_is_safe() {
    let err = StorageError::Encryption("internal detail".into());
    let msg = err.user_message();
    assert!(
        !msg.contains("internal"),
        "User message should not expose internal details"
    );
}

// ============================================================
// Replay-nonce row-corruption propagation
// (site 2 of _private/.../2026-05-21-silent-failures-in-security-paths)
//
// Pre-2026-05-23 `load_replay_nonces` discarded row read errors AND
// rows whose `nonce` BLOB was not 32 bytes via `.filter_map(|r| r.ok())`
// and `nonce_vec.try_into().ok()?`. A corrupted nonce row → empty set
// → ADR-029 replay defense window. The fix propagates both classes of
// error so storage faults surface loudly instead of opening a silent
// security hole.
// ============================================================

use vauchi_core::SymmetricKey;
use vauchi_core::storage::Storage;

// @internal
#[test]
fn load_replay_nonces_returns_err_when_nonce_blob_has_wrong_length() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    // Insert a malformed row directly: a 31-byte nonce (one byte short)
    // simulates either DB tampering or single-row corruption that the
    // current `nonce_vec.try_into().ok()?` silently filters out.
    storage
        .test_insert_malformed_replay_nonce("contact-1", &[0xAAu8; 31], 100)
        .expect("test helper insert should succeed");

    let result = storage.load_replay_nonces("contact-1");
    assert!(
        result.is_err(),
        "malformed replay-nonce row must surface as Err (ADR-029 replay defense), got {:?}",
        result
    );
}

// @internal
#[test]
fn load_replay_nonces_happy_path_returns_inserted_nonces_in_order() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    storage
        .save_replay_nonce("contact-1", &[0x11u8; 32], 100)
        .unwrap();
    storage
        .save_replay_nonce("contact-1", &[0x22u8; 32], 200)
        .unwrap();

    let nonces = storage.load_replay_nonces("contact-1").unwrap();
    assert_eq!(nonces.len(), 2, "both inserted nonces should load");
    assert_eq!(nonces[0], ([0x11u8; 32], 100));
    assert_eq!(nonces[1], ([0x22u8; 32], 200));
}

// ============================================================
// (site 8 of _private/.../2026-05-21-silent-failures-in-security-paths)
//
// `Storage::row_to_contact` used to swallow JSON deserialization errors
// for four trust-related columns:
//   - `exchange_transport` via `unwrap_or_default()` (line 213)
//   - `trust_metrics` via `.ok()` (line 230)
//   - `reciprocity` via `.ok()` (line 236)
//   - `confirmation_channel` via `.ok()` (line 241)
//
// `exchange_transport` is the ADR-034 trust-derivation input cited by the
// audit: a corrupt/garbled column silently degraded to `Qr` (the Default),
// producing a wrong trust badge. The other three are display-side
// enrichment — a corrupt row should still let the contact load (so the
// user can see their name + repair the row via re-exchange) but the
// failure must not be silent.
//
// Fix:
//   - `exchange_transport`: propagate as `StorageError::Serialization`
//     (load fails loudly; this is the trust-critical input).
//   - other three: surface via `tracing::warn!` then fall back to None.
// ============================================================

use vauchi_core::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::types::ExchangeTransport;

fn make_contact(name: &str) -> Contact {
    Contact::from_exchange(
        [name.as_bytes()[0]; 32],
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    )
}

// @internal
#[test]
fn load_contact_returns_err_when_exchange_transport_column_is_garbage() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut contact = make_contact("Alice");
    contact.set_exchange_transport(ExchangeTransport::Nfc);
    storage.save_contact(&contact).unwrap();

    // Corrupt the exchange_transport column to a value not in the
    // serde enum. Pre-fix the load silently fell back to Default (`Qr`)
    // and the contact appeared with a wrong trust badge.
    storage
        .test_corrupt_contact_text_column(contact.id(), "exchange_transport", "GarbageTransport!!")
        .expect("test helper should succeed");

    let result = storage.load_contact(contact.id());
    assert!(
        result.is_err(),
        "corrupt exchange_transport must surface as Err (ADR-034 trust-badge correctness), got {:?}",
        result
    );
}

// @internal
#[test]
fn load_contact_succeeds_with_logged_warning_when_trust_metrics_column_is_garbage() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let contact = make_contact("Bob");
    storage.save_contact(&contact).unwrap();

    storage
        .test_corrupt_contact_text_column(contact.id(), "trust_metrics", "not-valid-json{")
        .expect("test helper should succeed");

    // `trust_metrics` is enrichment; the load must NOT fail (the user
    // can still see Bob's name and re-exchange to repair the metrics).
    // The corruption is surfaced via tracing, not asserted in tests.
    let loaded = storage
        .load_contact(contact.id())
        .expect("trust_metrics corruption must not fail the load")
        .expect("contact must load");
    assert!(
        loaded.trust_metrics().is_none(),
        "trust_metrics must fall back to None when deser fails"
    );
}

// @internal
#[test]
fn load_contact_succeeds_with_logged_warning_when_reciprocity_column_is_garbage() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let contact = make_contact("Carol");
    storage.save_contact(&contact).unwrap();

    storage
        .test_corrupt_contact_text_column(contact.id(), "reciprocity", "not-a-reciprocity-enum")
        .expect("test helper should succeed");

    let loaded = storage
        .load_contact(contact.id())
        .expect("reciprocity corruption must not fail the load")
        .expect("contact must load");
    // Reciprocity defaults to the un-set value when corrupt; the loaded
    // contact's reciprocity equals what a freshly-constructed contact
    // (which never had set_reciprocity called) carries.
    let unset = make_contact("Dave");
    assert_eq!(
        loaded.reciprocity(0),
        unset.reciprocity(0),
        "reciprocity must fall back to the un-set value on deser error"
    );
}
