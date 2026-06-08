// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Performance threshold assertion tests.
//!
//! Enforces non-functional performance requirements from features/performance.feature.
//! These tests ARE the implementation — they codify threshold assertions.

use crate::common;

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
#[ignore = "wall-clock perf threshold: flaky under coverage/mutation instrumentation + machine load. Perf regressions are gated by the criterion `performance_regression` bench; these run in the `--ignored` nightly slow-test job. Kept out of the cargo-mutants baseline so it does not abort before testing mutants."]
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
#[ignore = "wall-clock perf threshold: flaky under coverage/mutation instrumentation + machine load. Perf regressions are gated by the criterion `performance_regression` bench; these run in the `--ignored` nightly slow-test job. Kept out of the cargo-mutants baseline so it does not abort before testing mutants."]
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
#[ignore = "wall-clock perf threshold: flaky under coverage/mutation instrumentation + machine load. Perf regressions are gated by the criterion `performance_regression` bench; these run in the `--ignored` nightly slow-test job. Kept out of the cargo-mutants baseline so it does not abort before testing mutants."]
fn test_query_under_50ms_with_1000_contacts() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key.clone()).unwrap();

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
            0,
        ))
        .unwrap();
        let shared = SymmetricKey::generate();
        let contact = Contact::from_exchange(pk, card, shared, 0);
        storage.save_contact(&contact).unwrap();
    }

    let start = Instant::now();
    let results = storage.search_contacts("User 05").unwrap();
    let elapsed = start.elapsed();

    // "User 05" matches User 050..User 059, User 050x..User 059x = 100 contacts
    assert_eq!(
        results.len(),
        100,
        "Search for 'User 05' should match exactly 100 contacts (050-059, 0500-0599)"
    );
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
#[ignore = "wall-clock perf threshold: flaky under coverage/mutation instrumentation + machine load. Perf regressions are gated by the criterion `performance_regression` bench; these run in the `--ignored` nightly slow-test job. Kept out of the cargo-mutants baseline so it does not abort before testing mutants."]
fn test_pagination_under_100ms_per_page() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key.clone()).unwrap();

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
        let contact = Contact::from_exchange(pk, card, shared, 0);
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
#[ignore = "wall-clock perf threshold: flaky under coverage/mutation instrumentation + machine load. Perf regressions are gated by the criterion `performance_regression` bench; these run in the `--ignored` nightly slow-test job. Kept out of the cargo-mutants baseline so it does not abort before testing mutants."]
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

/// List 1000 contacts from storage in under 500ms.
/// Traces to: features/performance.feature @contacts @scale
// @scenario: performance :: Handle 1000 contacts
#[test]
#[ignore = "wall-clock perf threshold: flaky under coverage/mutation instrumentation + machine load. Perf regressions are gated by the criterion `performance_regression` bench; these run in the `--ignored` nightly slow-test job. Kept out of the cargo-mutants baseline so it does not abort before testing mutants."]
fn test_list_1000_contacts_under_500ms() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key.clone()).unwrap();

    for i in 0..1000u32 {
        let pk = {
            let mut bytes = [0u8; 32];
            bytes[0..4].copy_from_slice(&i.to_be_bytes());
            bytes[4] = 0xBB;
            bytes
        };
        let card = ContactCard::new(&format!("Contact {:04}", i));
        let shared = SymmetricKey::generate();
        let contact = Contact::from_exchange(pk, card, shared, 0);
        storage.save_contact(&contact).unwrap();
    }

    let start = Instant::now();
    let contacts = storage.list_contacts().unwrap();
    let elapsed = start.elapsed();

    assert_eq!(contacts.len(), 1000, "Should load all 1000 contacts");
    assert!(
        elapsed < Duration::from_millis(500),
        "list_contacts(1000) took {:?}, expected < 500ms",
        elapsed
    );
}

/// List 100 contacts under 100ms — smooth UI scenario.
/// Traces to: features/performance.feature @contacts @scale
// @scenario: performance :: Handle 100 contacts smoothly
#[test]
#[ignore = "wall-clock perf threshold: flaky under coverage/mutation instrumentation + machine load. Perf regressions are gated by the criterion `performance_regression` bench; these run in the `--ignored` nightly slow-test job. Kept out of the cargo-mutants baseline so it does not abort before testing mutants."]
fn test_list_100_contacts_under_100ms() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key.clone()).unwrap();

    for i in 0..100u32 {
        let pk = {
            let mut bytes = [0u8; 32];
            bytes[0..4].copy_from_slice(&i.to_be_bytes());
            bytes[4] = 0xCC;
            bytes
        };
        let card = ContactCard::new(&format!("Person {:04}", i));
        let shared = SymmetricKey::generate();
        let contact = Contact::from_exchange(pk, card, shared, 0);
        storage.save_contact(&contact).unwrap();
    }

    let start = Instant::now();
    let contacts = storage.list_contacts().unwrap();
    let elapsed = start.elapsed();

    assert_eq!(contacts.len(), 100);
    assert!(
        elapsed < Duration::from_millis(100),
        "list_contacts(100) took {:?}, expected < 100ms",
        elapsed
    );
}

/// Queue and retrieve 100 pending updates under 2s.
/// Traces to: features/performance.feature @sync
// @scenario: performance :: Sync large batch of updates
#[test]
#[ignore = "wall-clock perf threshold: flaky under coverage/mutation instrumentation + machine load. Perf regressions are gated by the criterion `performance_regression` bench; these run in the `--ignored` nightly slow-test job. Kept out of the cargo-mutants baseline so it does not abort before testing mutants."]
fn test_queue_100_pending_updates_under_2s() {
    use vauchi_core::storage::{PendingUpdate, UpdateStatus};

    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key.clone()).unwrap();

    // Queue 100 pending updates (simulating 7-day offline accumulation)
    let start_queue = Instant::now();
    for i in 0..100u32 {
        let update = PendingUpdate {
            id: format!("update-{:04}", i),
            contact_id: format!("contact-{:04}", i % 20),
            update_type: "card_delta".to_string(),
            payload: vec![0xAB; 512], // ~512B encrypted delta
            created_at: 1700000000 + u64::from(i) * 3600,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: Some("https://relay.vauchi.app".to_string()),
        };
        storage.queue_update(&update).unwrap();
    }
    let queue_elapsed = start_queue.elapsed();

    let start_list = Instant::now();
    let pending = storage.get_all_pending_updates().unwrap();
    let list_elapsed = start_list.elapsed();

    assert_eq!(pending.len(), 100, "Should retrieve all 100 updates");
    let total = queue_elapsed + list_elapsed;
    assert!(
        total < Duration::from_secs(2),
        "Queue+list 100 updates took {:?}, expected < 2s",
        total
    );
}

/// 10 sequential ratchet encrypt/decrypt roundtrips complete under 500ms.
/// Traces to: features/performance.feature @stress
// @scenario: performance :: Handle many simultaneous operations
#[test]
#[ignore = "wall-clock perf threshold: flaky under coverage/mutation instrumentation + machine load. Perf regressions are gated by the criterion `performance_regression` bench; these run in the `--ignored` nightly slow-test job. Kept out of the cargo-mutants baseline so it does not abort before testing mutants."]
fn test_sequential_ratchet_operations() {
    use vauchi_core::crypto::ratchet::DoubleRatchetState;
    use vauchi_core::exchange::X3DHKeyPair;

    let mut results = Vec::with_capacity(10);
    let start = Instant::now();

    for _ in 0..10 {
        let bob_kp = X3DHKeyPair::generate();
        let shared = SymmetricKey::generate();

        let mut alice =
            DoubleRatchetState::initialize_initiator(&shared, *bob_kp.public_key()).unwrap();

        let msg = alice.encrypt(b"Hello from contact").unwrap();

        let mut bob = DoubleRatchetState::initialize_responder(&shared, bob_kp);

        let decrypted = bob.decrypt(&msg).unwrap();
        results.push(decrypted);
    }
    let elapsed = start.elapsed();

    // All 10 roundtrips must produce correct plaintext
    for result in &results {
        assert_eq!(result, b"Hello from contact");
    }
    assert!(
        elapsed < Duration::from_millis(500),
        "10 ratchet roundtrips took {:?}, expected < 500ms",
        elapsed
    );
}
