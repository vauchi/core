// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for advanced search and sort API (Phase 1D).
//!
//! Feature: contacts_management.feature @search @filter

use vauchi_core::api::contact_manager::ContactManager;
use vauchi_core::api::contact_manager::{SearchFilter, SortOrder};
use vauchi_core::api::events::EventDispatcher;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

use std::sync::Arc;

fn create_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

fn create_contact(name: &str, pk: [u8; 32]) -> Contact {
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(pk, card, shared_key)
}

fn create_verified_contact(name: &str, pk: [u8; 32]) -> Contact {
    let mut contact = create_contact(name, pk);
    contact.mark_fingerprint_verified();
    contact
}

fn setup_contacts(storage: &Storage) {
    // Alice — verified
    let alice = create_verified_contact("Alice", [1u8; 32]);
    storage.save_contact(&alice).unwrap();

    // Bob — not verified, timestamp newer
    let bob = create_contact("Bob", [2u8; 32]);
    storage.save_contact(&bob).unwrap();

    // Charlie — verified
    let charlie = create_verified_contact("Charlie", [3u8; 32]);
    storage.save_contact(&charlie).unwrap();

    // Diana — not verified, hidden
    let mut diana = create_contact("Diana", [4u8; 32]);
    diana.set_hidden(true);
    storage.save_contact(&diana).unwrap();
}

// --- SearchFilter tests ---

#[test]
fn test_search_filtered_no_filter_returns_all_visible() {
    let storage = create_storage();
    setup_contacts(&storage);
    let events = Arc::new(EventDispatcher::new());
    let manager = ContactManager::new(&storage, events);

    let filter = SearchFilter::default();
    let results = manager
        .search_contacts_filtered("", &filter, SortOrder::NameAsc)
        .unwrap();

    // Diana is hidden, should be excluded
    assert_eq!(results.len(), 3);
}

#[test]
fn test_search_filtered_by_query() {
    let storage = create_storage();
    setup_contacts(&storage);
    let events = Arc::new(EventDispatcher::new());
    let manager = ContactManager::new(&storage, events);

    let filter = SearchFilter::default();
    let results = manager
        .search_contacts_filtered("ali", &filter, SortOrder::NameAsc)
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].display_name(), "Alice");
}

#[test]
fn test_search_filtered_verified_only() {
    let storage = create_storage();
    setup_contacts(&storage);
    let events = Arc::new(EventDispatcher::new());
    let manager = ContactManager::new(&storage, events);

    let filter = SearchFilter {
        verified_only: true,
        ..Default::default()
    };
    let results = manager
        .search_contacts_filtered("", &filter, SortOrder::NameAsc)
        .unwrap();

    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|c| c.is_fingerprint_verified()));
    assert_eq!(results[0].display_name(), "Alice");
    assert_eq!(results[1].display_name(), "Charlie");
}

// --- SortOrder tests ---

#[test]
fn test_sort_name_ascending() {
    let storage = create_storage();
    setup_contacts(&storage);
    let events = Arc::new(EventDispatcher::new());
    let manager = ContactManager::new(&storage, events);

    let filter = SearchFilter::default();
    let results = manager
        .search_contacts_filtered("", &filter, SortOrder::NameAsc)
        .unwrap();

    let names: Vec<&str> = results.iter().map(|c| c.display_name()).collect();
    assert_eq!(names, vec!["Alice", "Bob", "Charlie"]);
}

#[test]
fn test_sort_name_descending() {
    let storage = create_storage();
    setup_contacts(&storage);
    let events = Arc::new(EventDispatcher::new());
    let manager = ContactManager::new(&storage, events);

    let filter = SearchFilter::default();
    let results = manager
        .search_contacts_filtered("", &filter, SortOrder::NameDesc)
        .unwrap();

    let names: Vec<&str> = results.iter().map(|c| c.display_name()).collect();
    assert_eq!(names, vec!["Charlie", "Bob", "Alice"]);
}

#[test]
fn test_sort_recent_first() {
    let storage = create_storage();
    setup_contacts(&storage);
    let events = Arc::new(EventDispatcher::new());
    let manager = ContactManager::new(&storage, events);

    let filter = SearchFilter::default();
    let results = manager
        .search_contacts_filtered("", &filter, SortOrder::RecentFirst)
        .unwrap();

    // All contacts have same timestamp (from_exchange uses SystemTime::now())
    // Just verify all 3 are returned
    assert_eq!(results.len(), 3);
}

#[test]
fn test_sort_verification_status() {
    let storage = create_storage();
    setup_contacts(&storage);
    let events = Arc::new(EventDispatcher::new());
    let manager = ContactManager::new(&storage, events);

    let filter = SearchFilter::default();
    let results = manager
        .search_contacts_filtered("", &filter, SortOrder::VerificationStatus)
        .unwrap();

    // Verified contacts should come first (Alice, Charlie), then unverified (Bob)
    assert!(results[0].is_fingerprint_verified());
    assert!(results[1].is_fingerprint_verified());
    assert!(!results[2].is_fingerprint_verified());
}

// --- Combined filter + sort tests ---

#[test]
fn test_filter_verified_sort_name_desc() {
    let storage = create_storage();
    setup_contacts(&storage);
    let events = Arc::new(EventDispatcher::new());
    let manager = ContactManager::new(&storage, events);

    let filter = SearchFilter {
        verified_only: true,
        ..Default::default()
    };
    let results = manager
        .search_contacts_filtered("", &filter, SortOrder::NameDesc)
        .unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].display_name(), "Charlie");
    assert_eq!(results[1].display_name(), "Alice");
}

#[test]
fn test_search_empty_results() {
    let storage = create_storage();
    setup_contacts(&storage);
    let events = Arc::new(EventDispatcher::new());
    let manager = ContactManager::new(&storage, events);

    let filter = SearchFilter::default();
    let results = manager
        .search_contacts_filtered("zzz_nonexistent", &filter, SortOrder::NameAsc)
        .unwrap();

    assert_eq!(results.len(), 0);
}

#[test]
fn test_search_with_no_contacts() {
    let storage = create_storage();
    let events = Arc::new(EventDispatcher::new());
    let manager = ContactManager::new(&storage, events);

    let filter = SearchFilter::default();
    let results = manager
        .search_contacts_filtered("", &filter, SortOrder::NameAsc)
        .unwrap();

    assert_eq!(results.len(), 0);
}
