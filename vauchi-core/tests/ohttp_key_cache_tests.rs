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
    storage.save_ohttp_key(relay, &key).unwrap();
    let cached = storage.load_ohttp_key(relay).unwrap();
    assert!(cached.is_some());
    let (bytes, fetched_at) = cached.unwrap();
    assert_eq!(bytes, key);
    assert!(fetched_at > 0);
}

// @scenario: sync:OHTTP key cache persistence
#[test]
fn test_ohttp_cache_returns_none_when_empty() {
    let storage = open_storage();
    let cached = storage.load_ohttp_key("https://no-such-relay.com").unwrap();
    assert!(cached.is_none());
}

// @scenario: sync:OHTTP key cache persistence
#[test]
fn test_ohttp_cache_clear() {
    let storage = open_storage();
    let relay = "https://relay.example.com";
    storage.save_ohttp_key(relay, &[1, 2, 3]).unwrap();
    storage.clear_ohttp_key(relay).unwrap();
    let cached = storage.load_ohttp_key(relay).unwrap();
    assert!(cached.is_none());
}

// @scenario: sync:OHTTP key cache persistence
#[test]
fn test_ohttp_cache_upsert_overwrites() {
    let storage = open_storage();
    let relay = "https://relay.example.com";
    storage.save_ohttp_key(relay, &[1, 2]).unwrap();
    storage.save_ohttp_key(relay, &[3, 4]).unwrap();
    let (bytes, _) = storage.load_ohttp_key(relay).unwrap().unwrap();
    assert_eq!(bytes, vec![3, 4]);
}
