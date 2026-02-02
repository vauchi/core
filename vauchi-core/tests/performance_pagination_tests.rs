// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Performance: Pagination and Search Tests
//!
//! Feature file: features/performance.feature @pagination @search
//! Tests for paginated contact listing and SQL-level search.

mod common;

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

/// Creates a test contact with a unique name and public key.
fn create_numbered_contact(n: usize) -> Contact {
    let mut card = ContactCard::new(&format!("Contact {:04}", n));
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        &format!("contact{}@example.com", n),
    ))
    .unwrap();
    // Use a deterministic "public key" derived from the number
    let mut pk = [0u8; 32];
    pk[..8].copy_from_slice(&(n as u64).to_be_bytes());
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(pk, card, shared_key)
}

/// Helper: populate storage with N contacts.
fn populate_contacts(storage: &Storage, count: usize) {
    for i in 0..count {
        let contact = create_numbered_contact(i);
        storage.save_contact(&contact).unwrap();
    }
}

// ============================================================
// Pagination Tests
// ============================================================

#[test]
fn test_list_contacts_paginated_returns_correct_subset() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    populate_contacts(&storage, 100);

    // Page 1: offset=0, limit=10
    let page1 = storage.list_contacts_paginated(0, 10).unwrap();
    assert_eq!(page1.len(), 10, "First page should have 10 contacts");

    // Page 2: offset=10, limit=10
    let page2 = storage.list_contacts_paginated(10, 10).unwrap();
    assert_eq!(page2.len(), 10, "Second page should have 10 contacts");

    // Pages should not overlap
    let page1_ids: Vec<String> = page1.iter().map(|c| c.id().to_string()).collect();
    let page2_ids: Vec<String> = page2.iter().map(|c| c.id().to_string()).collect();
    for id in &page2_ids {
        assert!(!page1_ids.contains(id), "Pages should not overlap");
    }
}

#[test]
fn test_list_contacts_paginated_respects_ordering() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    populate_contacts(&storage, 50);

    let page = storage.list_contacts_paginated(0, 50).unwrap();
    // Should be ordered by display_name
    for i in 1..page.len() {
        assert!(
            page[i - 1].display_name() <= page[i].display_name(),
            "Contacts should be ordered by display_name"
        );
    }
}

#[test]
fn test_list_contacts_paginated_last_page_partial() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    populate_contacts(&storage, 25);

    // Request page starting at offset 20 with limit 10
    let page = storage.list_contacts_paginated(20, 10).unwrap();
    assert_eq!(page.len(), 5, "Last page should have remaining contacts");
}

#[test]
fn test_list_contacts_paginated_beyond_range_returns_empty() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    populate_contacts(&storage, 10);

    let page = storage.list_contacts_paginated(100, 10).unwrap();
    assert!(page.is_empty(), "Beyond-range offset should return empty");
}

#[test]
fn test_list_contacts_paginated_zero_limit_returns_empty() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    populate_contacts(&storage, 10);

    let page = storage.list_contacts_paginated(0, 0).unwrap();
    assert!(page.is_empty(), "Zero limit should return empty");
}

#[test]
fn test_list_contacts_paginated_all_contacts_across_pages() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    populate_contacts(&storage, 53);

    let mut all_ids = Vec::new();
    let page_size = 10;
    let mut offset = 0;
    loop {
        let page = storage.list_contacts_paginated(offset, page_size).unwrap();
        if page.is_empty() {
            break;
        }
        for c in &page {
            all_ids.push(c.id().to_string());
        }
        offset += page_size;
    }

    assert_eq!(
        all_ids.len(),
        53,
        "All contacts should be retrievable across pages"
    );
    // No duplicates
    let unique: std::collections::HashSet<_> = all_ids.iter().collect();
    assert_eq!(unique.len(), 53, "No duplicate contacts across pages");
}

// ============================================================
// Search Tests
// ============================================================

#[test]
fn test_search_contacts_by_name() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    populate_contacts(&storage, 100);

    // Search for "Contact 0042"
    let results = storage.search_contacts("0042").unwrap();
    assert_eq!(results.len(), 1, "Should find exactly one match for '0042'");
    assert_eq!(results[0].display_name(), "Contact 0042");
}

#[test]
fn test_search_contacts_partial_match() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    populate_contacts(&storage, 100);

    // Search for "Contact 00" should match Contact 0000..0009
    let results = storage.search_contacts("Contact 00").unwrap();
    assert!(
        results.len() >= 10,
        "Partial match should return multiple results, got {}",
        results.len()
    );
}

#[test]
fn test_search_contacts_case_insensitive() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    populate_contacts(&storage, 10);

    let results = storage.search_contacts("contact").unwrap();
    assert_eq!(results.len(), 10, "Search should be case-insensitive");
}

#[test]
fn test_search_contacts_no_match_returns_empty() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    populate_contacts(&storage, 10);

    let results = storage.search_contacts("zzz_nonexistent").unwrap();
    assert!(results.is_empty(), "Non-matching query should return empty");
}

#[test]
fn test_search_contacts_empty_query_returns_all() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    populate_contacts(&storage, 10);

    let results = storage.search_contacts("").unwrap();
    assert_eq!(results.len(), 10, "Empty query should return all contacts");
}

#[test]
fn test_search_contacts_with_1000_contacts() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    populate_contacts(&storage, 1000);

    let results = storage.search_contacts("Contact 0500").unwrap();
    assert_eq!(
        results.len(),
        1,
        "Should find exact match among 1000 contacts"
    );
    assert_eq!(results[0].display_name(), "Contact 0500");
}

// ============================================================
// Contact Count with Pagination
// ============================================================

#[test]
fn test_count_contacts_for_pagination() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    populate_contacts(&storage, 150);

    let total = storage.count_contacts().unwrap();
    assert_eq!(total, 150);

    // Calculate total pages with page size 50
    let page_size = 50usize;
    let total_pages = total.div_ceil(page_size);
    assert_eq!(total_pages, 3);
}
