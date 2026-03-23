// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Performance threshold assertion tests.
//!
//! Enforces non-functional performance requirements from features/performance.feature.
//! These tests ARE the implementation — they codify threshold assertions.

mod common;

use std::time::{Duration, Instant};
use tempfile::NamedTempFile;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

/// Encrypt 64KB payload in under 100ms.
/// Traces to: features/performance.feature @resources
// @scenario: performance :: Efficient cryptographic operations
#[test]
fn test_encryption_under_100ms() {
    let key = SymmetricKey::generate();
    let plaintext = vec![0xABu8; 64 * 1024]; // 64KB

    let start = Instant::now();
    let ciphertext = vauchi_core::crypto::encrypt(&key, &plaintext).unwrap();
    let elapsed = start.elapsed();

    assert!(!ciphertext.is_empty());
    assert!(
        elapsed < Duration::from_millis(100),
        "Encryption took {:?}, expected < 100ms",
        elapsed
    );
}

/// Decrypt 64KB payload in under 100ms.
/// Traces to: features/performance.feature @resources
// @scenario: performance :: Efficient cryptographic operations
#[test]
fn test_decryption_under_100ms() {
    let key = SymmetricKey::generate();
    let plaintext = vec![0xCDu8; 64 * 1024]; // 64KB
    let ciphertext = vauchi_core::crypto::encrypt(&key, &plaintext).unwrap();

    let start = Instant::now();
    let decrypted = vauchi_core::crypto::decrypt(&key, &ciphertext).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(decrypted, plaintext);
    assert!(
        elapsed < Duration::from_millis(100),
        "Decryption took {:?}, expected < 100ms",
        elapsed
    );
}

/// Search 1000 contacts via search_contacts() in under 50ms.
/// Traces to: features/performance.feature @resources
// @scenario: performance :: Efficient database queries
// @scenario: performance :: Search performance with many contacts
#[test]
fn test_query_under_50ms_with_1000_contacts() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key.clone()).unwrap();

    // Insert 1000 contacts with properly encrypted data
    for i in 0..1000u32 {
        let pk = {
            let mut bytes = [0u8; 32];
            bytes[0] = (i >> 8) as u8;
            bytes[1] = (i & 0xFF) as u8;
            bytes[2] = 0x42;
            bytes
        };
        let mut card = ContactCard::new(&format!("User {:04}", i));
        card.add_field(vauchi_core::contact_card::ContactField::new(
            vauchi_core::contact_card::FieldType::Email,
            "Email",
            &format!("user{}@example.com", i),
        ))
        .unwrap();
        let shared = SymmetricKey::generate();
        let contact = Contact::from_exchange(pk, card, shared);
        storage.save_contact(&contact).unwrap();
    }

    let start = Instant::now();
    let results = storage.search_contacts("User 05").unwrap();
    let elapsed = start.elapsed();

    assert!(!results.is_empty(), "Search should return results");
    assert!(
        elapsed < Duration::from_millis(50),
        "Query took {:?}, expected < 50ms",
        elapsed
    );
}

/// Paginate 500 contacts in pages of 50 — each page under 100ms.
/// Traces to: features/performance.feature @pagination
// @scenario: performance :: Batch contact loading with pagination
#[test]
fn test_pagination_under_100ms_per_page() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key.clone()).unwrap();

    // Insert 500 contacts
    for i in 0..500u32 {
        let pk = {
            let mut bytes = [0u8; 32];
            bytes[0] = (i >> 8) as u8;
            bytes[1] = (i & 0xFF) as u8;
            bytes[3] = 0xAA;
            bytes
        };
        let card = ContactCard::new(&format!("Contact {:04}", i));
        let shared = SymmetricKey::generate();
        let contact = Contact::from_exchange(pk, card, shared);
        storage.save_contact(&contact).unwrap();
    }

    let page_size = 50;
    let num_pages = 500 / page_size;

    for page in 0..num_pages {
        let offset = page * page_size;
        let start = Instant::now();
        let results = storage.list_contacts_paginated(offset, page_size).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(
            results.len(),
            page_size,
            "Page {} should return {} contacts",
            page,
            page_size
        );
        assert!(
            elapsed < Duration::from_millis(100),
            "Page {} took {:?}, expected < 100ms",
            page,
            elapsed
        );
    }
}

/// Open a file-based Storage in under 500ms.
/// Traces to: features/performance.feature @startup
// @scenario: performance :: Cold start time
#[test]
fn test_storage_open_under_500ms() {
    let tmp = NamedTempFile::new().unwrap();

    let start = Instant::now();
    let _storage = Storage::open(tmp.path(), SymmetricKey::generate()).unwrap();
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(500),
        "Storage::open took {:?}, expected < 500ms",
        elapsed
    );
}
