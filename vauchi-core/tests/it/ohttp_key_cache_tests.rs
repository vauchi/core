// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for OHTTP key cache storage.

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

fn open_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

// @scenario: sync:OHTTP key cache persistence
#[test]
fn test_ohttp_cache_save_and_load() {
    let storage = open_storage();
    let relay = "https://relay.example.com";
    let key = vec![1, 2, 3, 4];
    storage.ohttp_cache().save_ohttp_key(relay, &key).unwrap();
    let cached = storage.ohttp_cache().load_ohttp_key(relay).unwrap();
    assert!(cached.is_some());
    let (bytes, fetched_at) = cached.unwrap();
    assert_eq!(bytes, key);
    assert!(fetched_at > 0);
}

// @scenario: sync:OHTTP key cache persistence
#[test]
fn test_ohttp_cache_returns_none_when_empty() {
    let storage = open_storage();
    let cached = storage
        .ohttp_cache()
        .load_ohttp_key("https://no-such-relay.com")
        .unwrap();
    assert!(cached.is_none());
}

// @scenario: sync:OHTTP key cache persistence
#[test]
fn test_ohttp_cache_clear() {
    let storage = open_storage();
    let relay = "https://relay.example.com";
    storage
        .ohttp_cache()
        .save_ohttp_key(relay, &[1, 2, 3])
        .unwrap();
    storage.ohttp_cache().clear_ohttp_key(relay).unwrap();
    let cached = storage.ohttp_cache().load_ohttp_key(relay).unwrap();
    assert!(cached.is_none());
}

// @scenario: sync:OHTTP key cache persistence
#[test]
fn test_ohttp_cache_upsert_overwrites() {
    let storage = open_storage();
    let relay = "https://relay.example.com";
    storage
        .ohttp_cache()
        .save_ohttp_key(relay, &[1, 2])
        .unwrap();
    storage
        .ohttp_cache()
        .save_ohttp_key(relay, &[3, 4])
        .unwrap();
    let (bytes, _) = storage
        .ohttp_cache()
        .load_ohttp_key(relay)
        .unwrap()
        .unwrap();
    assert_eq!(bytes, vec![3, 4]);
}

/// W-6: Clearing one relay's key must not affect other relays.
///
/// Verifies that the cache key includes the relay URL so that entries
/// are truly isolated — a stale-key eviction for relay-A never silently
/// removes relay-B's still-valid key.
// @scenario: ohttp_cache :: clearing one relay key preserves others
#[test]
fn test_ohttp_cache_clear_does_not_affect_other_relays() {
    let storage = open_storage();

    storage
        .ohttp_cache()
        .save_ohttp_key("https://relay-a.example.com", &[1, 2])
        .unwrap();
    storage
        .ohttp_cache()
        .save_ohttp_key("https://relay-b.example.com", &[3, 4])
        .unwrap();

    storage
        .ohttp_cache()
        .clear_ohttp_key("https://relay-a.example.com")
        .unwrap();

    // relay-b must be unaffected.
    let b = storage
        .ohttp_cache()
        .load_ohttp_key("https://relay-b.example.com")
        .unwrap();
    assert!(
        b.is_some(),
        "relay-b key must still be present after clearing relay-a"
    );
    assert_eq!(
        b.unwrap().0,
        vec![3u8, 4],
        "relay-b key bytes must be unchanged"
    );

    // relay-a must be gone.
    let a = storage
        .ohttp_cache()
        .load_ohttp_key("https://relay-a.example.com")
        .unwrap();
    assert!(
        a.is_none(),
        "relay-a key must be absent after clear_ohttp_key"
    );
}

// ── Held signed key per relay (#288 plan 6.3) ───────────────────────

use vauchi_core::network::ohttp_key_trust::HeldOhttpKey;

fn held(window: u64, body: u8) -> HeldOhttpKey {
    HeldOhttpKey {
        window,
        key_config: vec![body; 41],
    }
}

// @internal
#[test]
fn a_held_key_is_kept_per_relay_with_its_window() {
    let storage = open_storage();
    let store = storage.ohttp_cache();

    store
        .save_held_ohttp_key("https://one.example", &held(20_367, 1))
        .unwrap();
    store
        .save_held_ohttp_key("https://two.example", &held(20_366, 2))
        .unwrap();

    assert_eq!(
        store.load_held_ohttp_key("https://one.example").unwrap(),
        Some(held(20_367, 1))
    );
    assert_eq!(
        store.load_held_ohttp_key("https://two.example").unwrap(),
        Some(held(20_366, 2))
    );
    assert_eq!(
        store.load_held_ohttp_key("https://three.example").unwrap(),
        None
    );
}

// @internal
#[test]
fn a_newer_held_key_replaces_the_old_one() {
    let storage = open_storage();
    let store = storage.ohttp_cache();
    store
        .save_held_ohttp_key("https://one.example", &held(20_367, 1))
        .unwrap();

    store
        .save_held_ohttp_key("https://one.example", &held(20_368, 3))
        .unwrap();

    assert_eq!(
        store.load_held_ohttp_key("https://one.example").unwrap(),
        Some(held(20_368, 3))
    );
}

/// The held key and the unsigned cache are separate: clearing the cache
/// after a stale-key rejection must not forget which window the client
/// already holds, or an older record could be accepted again.
// @internal
#[test]
fn clearing_the_unsigned_cache_keeps_the_held_key() {
    let storage = open_storage();
    let store = storage.ohttp_cache();
    store
        .save_held_ohttp_key("https://one.example", &held(20_367, 1))
        .unwrap();
    store
        .save_ohttp_key("https://one.example", &[9, 9])
        .unwrap();

    store.clear_ohttp_key("https://one.example").unwrap();

    assert_eq!(
        store.load_held_ohttp_key("https://one.example").unwrap(),
        Some(held(20_367, 1))
    );
}
