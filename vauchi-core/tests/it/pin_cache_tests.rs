// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for certificate pin cache storage.

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::PinnedCertificate;
use vauchi_core::storage::Storage;

fn open_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

// @scenario: pinning:pin cache persistence
// @internal
#[test]
fn save_and_load_pin_cache() {
    let storage = open_storage();
    let relay = "https://relay.vauchi.app";
    let pins = vec![
        PinnedCertificate::new([0xAA; 32]),
        PinnedCertificate::new([0xBB; 32]),
    ];

    storage.pin_cache().save_pin_cache(relay, &pins).unwrap();
    let cached = storage.pin_cache().load_pin_cache(relay).unwrap();

    assert!(cached.is_some(), "cached pins must be present after save");
    let (loaded_pins, fetched_at) = cached.unwrap();
    assert_eq!(loaded_pins.len(), 2, "must load same number of pins");
    assert_eq!(loaded_pins[0], pins[0]);
    assert_eq!(loaded_pins[1], pins[1]);
    assert!(fetched_at > 0, "fetched_at must be a valid timestamp");
}

// @scenario: pinning:pin cache persistence
// @internal
#[test]
fn load_pin_cache_returns_none_when_empty() {
    let storage = open_storage();
    let cached = storage
        .pin_cache()
        .load_pin_cache("https://no-such-relay.com")
        .unwrap();
    assert!(cached.is_none());
}

// @scenario: pinning:pin cache persistence
// @internal
#[test]
fn clear_pin_cache_removes_entry() {
    let storage = open_storage();
    let relay = "https://relay.vauchi.app";
    storage
        .pin_cache()
        .save_pin_cache(relay, &[PinnedCertificate::new([0xCC; 32])])
        .unwrap();

    storage.pin_cache().clear_pin_cache(relay).unwrap();

    let cached = storage.pin_cache().load_pin_cache(relay).unwrap();
    assert!(cached.is_none(), "cache must be empty after clear");
}

// @scenario: pinning:pin cache persistence
// @internal
#[test]
fn save_pin_cache_upserts() {
    let storage = open_storage();
    let relay = "https://relay.vauchi.app";

    let pins_v1 = vec![PinnedCertificate::new([0x11; 32])];
    let pins_v2 = vec![
        PinnedCertificate::new([0x22; 32]),
        PinnedCertificate::new([0x33; 32]),
    ];

    storage.pin_cache().save_pin_cache(relay, &pins_v1).unwrap();
    storage.pin_cache().save_pin_cache(relay, &pins_v2).unwrap();

    let (loaded, _) = storage.pin_cache().load_pin_cache(relay).unwrap().unwrap();
    assert_eq!(loaded.len(), 2, "upsert must replace with latest pins");
    assert_eq!(loaded[0], pins_v2[0]);
    assert_eq!(loaded[1], pins_v2[1]);
}

// @scenario: pinning:pin cache persistence
// @internal
#[test]
fn clear_pin_cache_does_not_affect_other_relays() {
    let storage = open_storage();

    storage
        .pin_cache()
        .save_pin_cache(
            "https://relay-a.example.com",
            &[PinnedCertificate::new([0xAA; 32])],
        )
        .unwrap();
    storage
        .pin_cache()
        .save_pin_cache(
            "https://relay-b.example.com",
            &[PinnedCertificate::new([0xBB; 32])],
        )
        .unwrap();

    storage
        .pin_cache()
        .clear_pin_cache("https://relay-a.example.com")
        .unwrap();

    let b = storage
        .pin_cache()
        .load_pin_cache("https://relay-b.example.com")
        .unwrap();
    assert!(b.is_some(), "relay-b pins must survive relay-a clear");

    let a = storage
        .pin_cache()
        .load_pin_cache("https://relay-a.example.com")
        .unwrap();
    assert!(a.is_none(), "relay-a pins must be gone after clear");
}

// @scenario: pinning:empty pin list roundtrip
// Note: the transport layer (fetch_pin_config) rejects empty responses,
// but the storage layer must handle empty lists gracefully in case
// a future caller has a legitimate reason to clear cached pins.
// @internal
#[test]
fn save_empty_pin_list_roundtrips() {
    let storage = open_storage();
    let relay = "https://relay.vauchi.app";

    storage.pin_cache().save_pin_cache(relay, &[]).unwrap();
    let (loaded, _) = storage.pin_cache().load_pin_cache(relay).unwrap().unwrap();
    assert!(loaded.is_empty(), "empty pin list must roundtrip");
}
