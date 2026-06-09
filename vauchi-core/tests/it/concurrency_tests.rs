// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Concurrency Tests for SQLite Storage
//!
//! These tests verify thread safety and concurrent access patterns for the
//! Storage module. SQLite connections themselves aren't Sync, but we test:
//! 1. Sequential operations remain consistent
//! 2. Multiple connections to the same file work correctly
//! 3. Read-after-write consistency
//! 4. WAL mode concurrent access (if enabled)

use rand::Rng;
use std::thread;
use tempfile::tempdir;
use vauchi_core::contact::Contact;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;
use vauchi_core::{ContactCard, ContactField, FieldType};

// =============================================================================
// =============================================================================

/// Open storage with retries for CI environments where heavy parallel test
/// execution creates transient SQLITE_BUSY errors during initialization.
/// SQLite's busy_timeout handles lock contention for SQL statements, but
/// Storage::open also runs DDL (CREATE TABLE IF NOT EXISTS) and maintenance
/// (DELETE) that can fail under extreme I/O pressure.
fn open_with_retry(path: &std::path::Path, key: SymmetricKey, max_retries: u32) -> Storage {
    for attempt in 0..=max_retries {
        match Storage::open(path, key.clone()) {
            Ok(storage) => return storage,
            Err(e) if attempt < max_retries => {
                // nosemgrep: vauchi-cc06-no-sleep-in-tests — retry backoff for real SQLITE_BUSY, not test timing
                std::thread::sleep(std::time::Duration::from_millis(200 * (attempt as u64 + 1)));
            }
            Err(e) => panic!("Storage::open failed after {} retries: {}", max_retries, e),
        }
    }
    unreachable!()
}

fn create_test_contact(name: &str) -> Contact {
    let mut card = ContactCard::new(name);
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        &format!("{}@example.com", name.to_lowercase().replace(' ', ".")),
        0,
    ))
    .unwrap();

    // Generate a random public key so each contact has a unique ID
    let mut public_key = [0u8; 32];
    rand::thread_rng().fill(&mut public_key);

    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key, 0)
}

// =============================================================================
// =============================================================================

// @internal
#[test]
fn test_sequential_contact_operations() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    let mut contact_ids = Vec::new();

    for i in 0..100 {
        let contact = create_test_contact(&format!("User {}", i));
        let id = contact.id().to_string();
        storage.contacts().save_contact(&contact).unwrap();
        contact_ids.push(id);
    }

    let contacts = storage.contacts().list_contacts().unwrap();
    assert_eq!(contacts.len(), 100);

    for id in &contact_ids {
        let loaded = storage.contacts().load_contact(id).unwrap();
        assert!(loaded.is_some(), "expected Some value");
    }

    for id in contact_ids.iter().take(50) {
        storage.delete_contact(id).unwrap();
    }

    let remaining = storage.contacts().list_contacts().unwrap();
    assert_eq!(remaining.len(), 50);
}

// @internal
#[test]
fn test_sequential_pending_update_operations() {
    use vauchi_core::storage::{PendingUpdate, UpdateStatus};

    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    for i in 0..50 {
        let update = PendingUpdate {
            id: format!("update-{}", i),
            contact_id: format!("contact-{}", i % 10),
            update_type: "card_delta".to_string(),
            payload: vec![i as u8; 100],
            created_at: 1700000000 + i as u64,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: None,
        };
        storage.pending().queue_update(&update).unwrap();
    }

    let updates = storage.pending().get_all_pending_updates().unwrap();
    assert_eq!(updates.len(), 50);

    // Mark some as sent (delete)
    for i in 0..25 {
        storage
            .pending()
            .mark_update_sent(&format!("update-{}", i))
            .unwrap();
    }

    let remaining = storage.pending().get_all_pending_updates().unwrap();
    assert_eq!(remaining.len(), 25);
}

// =============================================================================
// =============================================================================

// @internal
#[test]
fn test_multiple_connections_same_file() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("concurrent.db");

    let key = SymmetricKey::generate();

    {
        let storage1 = Storage::open(&db_path, key.clone()).unwrap();
        let contact = create_test_contact("Alice");
        storage1.contacts().save_contact(&contact).unwrap();
    }

    {
        let storage2 = Storage::open(&db_path, key.clone()).unwrap();
        let contacts = storage2.contacts().list_contacts().unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].card().display_name(), "Alice");
    }

    {
        let storage3 = Storage::open(&db_path, key.clone()).unwrap();
        let contact = create_test_contact("Bob");
        storage3.contacts().save_contact(&contact).unwrap();
    }

    {
        let storage4 = Storage::open(&db_path, key).unwrap();
        let contacts = storage4.contacts().list_contacts().unwrap();
        assert_eq!(contacts.len(), 2);
    }
}

// @internal
#[test]
fn test_concurrent_readers_file_based() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("readers.db");

    let key = SymmetricKey::generate();

    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        for i in 0..10 {
            let contact = create_test_contact(&format!("Contact {}", i));
            storage.contacts().save_contact(&contact).unwrap();
        }
    }

    // Spawn multiple reader threads — no Barrier synchronization.
    // WAL mode handles concurrent reads correctly; a Barrier would risk
    // deadlock if any thread panics during Storage::open (the remaining
    // threads wait at the barrier forever, causing a 180s nextest timeout).
    let path = db_path.clone();
    let mut handles = Vec::new();

    for thread_id in 0..5 {
        let thread_path = path.clone();
        let thread_key = key.clone();

        let handle = thread::spawn(move || {
            let storage = open_with_retry(&thread_path, thread_key, 5);

            let contacts = storage.contacts().list_contacts().unwrap();

            assert_eq!(contacts.len(), 10, "Thread {} saw wrong count", thread_id);

            for contact in &contacts {
                let loaded = storage.contacts().load_contact(contact.id()).unwrap();
                assert!(loaded.is_some(), "expected Some value");
            }

            thread_id
        });
        handles.push(handle);
    }

    for handle in handles {
        let thread_id = handle.join().expect("Thread panicked");
        assert!(thread_id < 5);
    }
}

// @internal
#[test]
fn test_sequential_writers_file_based() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("writers.db");

    let key = SymmetricKey::generate();

    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        let _ = storage.contacts().list_contacts().unwrap(); // Just init
    }

    // Writers from different threads — opens are retried to handle
    // SQLite contention during concurrent initialization
    let path = db_path.clone();
    let mut handles = Vec::new();

    for thread_id in 0..5 {
        let thread_path = path.clone();
        let thread_key = key.clone();

        let handle = thread::spawn(move || {
            let storage = open_with_retry(&thread_path, thread_key, 5);

            for i in 0..10 {
                let contact = create_test_contact(&format!("Thread{}Contact{}", thread_id, i));
                storage.contacts().save_contact(&contact).unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    let storage = Storage::open(&db_path, key).unwrap();
    let contacts = storage.contacts().list_contacts().unwrap();

    assert_eq!(contacts.len(), 50);
}

// =============================================================================
// =============================================================================

// @internal
#[test]
fn test_read_after_write_consistency() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("consistency.db");

    let key = SymmetricKey::generate();

    let contact = create_test_contact("Consistency Test");
    let contact_id = contact.id().to_string();
    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        storage.contacts().save_contact(&contact).unwrap();
    }

    // Immediately read from new connection
    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        let loaded = storage.contacts().load_contact(&contact_id).unwrap();
        assert!(loaded.is_some(), "expected Some value");
        assert_eq!(loaded.unwrap().card().display_name(), "Consistency Test");
    }
}

// @internal
#[test]
fn test_update_visibility_consistency() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("visibility.db");

    let key = SymmetricKey::generate();

    let contact = create_test_contact("Visibility Test");
    let contact_id = contact.id().to_string();

    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        storage.contacts().save_contact(&contact).unwrap();
    }

    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        let card = ContactCard::new("Updated Name");
        storage.contacts().save_own_card(&card).unwrap();
    }

    {
        let storage = Storage::open(&db_path, key).unwrap();

        let loaded = storage.contacts().load_contact(&contact_id).unwrap();
        assert!(loaded.is_some(), "expected Some value");

        let own_card = storage.contacts().load_own_card().unwrap();
        assert!(own_card.is_some(), "expected Some value");
        assert_eq!(own_card.unwrap().display_name(), "Updated Name");
    }
}

// =============================================================================
// =============================================================================

// @internal
#[test]
fn test_rapid_open_close_cycles() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("rapid.db");

    let key = SymmetricKey::generate();

    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        let contact = create_test_contact("Initial");
        storage.contacts().save_contact(&contact).unwrap();
    }

    // Rapid open/read/close cycles with retry for SQLite contention
    for i in 0..50 {
        let storage = open_with_retry(&db_path, key.clone(), 5);
        let contacts = storage.contacts().list_contacts().unwrap();
        assert!(!contacts.is_empty(), "Iteration {} found no contacts", i);
    }
}

// @internal
#[test]
fn test_interleaved_reads_writes() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("interleaved.db");

    let key = SymmetricKey::generate();

    {
        let _ = Storage::open(&db_path, key.clone()).unwrap();
    }

    // Interleaved read-write operations with retry for SQLite contention
    for i in 0..20 {
        {
            let storage = open_with_retry(&db_path, key.clone(), 5);
            let contact = create_test_contact(&format!("Contact {}", i));
            storage.contacts().save_contact(&contact).unwrap();
        }

        {
            let storage = open_with_retry(&db_path, key.clone(), 5);
            let contacts = storage.contacts().list_contacts().unwrap();
            assert_eq!(contacts.len(), i + 1, "Wrong count after iteration {}", i);
        }
    }
}

// =============================================================================
// ERROR HANDLING UNDER CONCURRENT ACCESS
// =============================================================================

// @internal
#[test]
fn test_delete_nonexistent_is_idempotent() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    let result = storage.delete_contact("does-not-exist");
    assert!(result.is_ok(), "expected success");

    for _ in 0..10 {
        let result = storage.delete_contact("still-does-not-exist");
        assert!(result.is_ok(), "expected success");
    }
}

// @internal
#[test]
fn test_double_save_overwrites() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    let mut card = ContactCard::new("Original Name");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "original@example.com",
        0,
    ))
    .unwrap();
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([1u8; 32], card, shared_key, 0);
    let id = contact.id().to_string();

    storage.contacts().save_contact(&contact).unwrap();

    let loaded = storage.contacts().load_contact(&id).unwrap().unwrap();
    assert_eq!(loaded.card().display_name(), "Original Name");

    let mut card2 = ContactCard::new("Updated Name");
    card2
        .add_field(ContactField::new(
            FieldType::Email,
            "Work",
            "updated@example.com",
            0,
        ))
        .unwrap();
    let shared_key2 = SymmetricKey::generate();
    let contact2 = Contact::from_exchange([1u8; 32], card2, shared_key2, 0);

    // Save should overwrite (upsert behavior)
    storage.contacts().save_contact(&contact2).unwrap();

    let loaded2 = storage.contacts().load_contact(&id).unwrap().unwrap();
    assert_eq!(loaded2.card().display_name(), "Updated Name");
}
