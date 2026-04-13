// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for shared name add/remove/list operations.
//!
//! @scenario: contacts_management.feature - Shared name management

use vauchi_core::{Contact, ContactCard, SymmetricKey, Vauchi};

fn setup_with_contact() -> (Vauchi, String) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    let mut pk = [0u8; 32];
    pk[0] = 1;
    let card = ContactCard::new("Bob");
    let contact = Contact::from_exchange(pk, card, SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    (wb, contact_id)
}

#[test]
fn test_add_and_list_shared_name() {
    let (wb, cid) = setup_with_contact();
    wb.add_contact_shared_name(&cid, "Bobby", false).unwrap();
    let names = wb.list_contact_shared_names(&cid).unwrap();
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].name, "Bobby");
    assert!(!names[0].is_primary);
}

#[test]
fn test_add_primary_name() {
    let (wb, cid) = setup_with_contact();
    wb.add_contact_shared_name(&cid, "Robert", true).unwrap();
    let names = wb.list_contact_shared_names(&cid).unwrap();
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].name, "Robert");
    assert!(names[0].is_primary);
}

#[test]
fn test_add_multiple_names() {
    let (wb, cid) = setup_with_contact();
    wb.add_contact_shared_name(&cid, "Bobby", true).unwrap();
    wb.add_contact_shared_name(&cid, "Rob", false).unwrap();
    wb.add_contact_shared_name(&cid, "Bob", false).unwrap();
    let names = wb.list_contact_shared_names(&cid).unwrap();
    assert_eq!(names.len(), 3, "Expected 3 shared names");
}

#[test]
fn test_remove_shared_name() {
    let (wb, cid) = setup_with_contact();
    wb.add_contact_shared_name(&cid, "Bobby", true).unwrap();
    wb.add_contact_shared_name(&cid, "Rob", false).unwrap();
    wb.remove_contact_shared_name(&cid, "Rob").unwrap();
    let names = wb.list_contact_shared_names(&cid).unwrap();
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].name, "Bobby");
}

#[test]
fn test_dedup_on_same_name() {
    let (wb, cid) = setup_with_contact();
    wb.add_contact_shared_name(&cid, "Bobby", true).unwrap();
    wb.add_contact_shared_name(&cid, "Bobby", false).unwrap();
    let names = wb.list_contact_shared_names(&cid).unwrap();
    assert_eq!(names.len(), 1, "Duplicate names must be deduplicated");
    assert_eq!(names[0].name, "Bobby");
}

#[test]
fn test_list_empty_when_none() {
    let (wb, cid) = setup_with_contact();
    let names = wb.list_contact_shared_names(&cid).unwrap();
    assert!(
        names.is_empty(),
        "No shared names added — list must be empty"
    );
}

#[test]
fn test_primary_name_listed_first() {
    let (wb, cid) = setup_with_contact();
    wb.add_contact_shared_name(&cid, "Rob", false).unwrap();
    wb.add_contact_shared_name(&cid, "Bobby", true).unwrap();
    wb.add_contact_shared_name(&cid, "Bob", false).unwrap();
    let names = wb.list_contact_shared_names(&cid).unwrap();
    assert!(!names.is_empty(), "Expected at least one shared name");
    assert!(
        names[0].is_primary,
        "Primary name must appear first; got: {:?}",
        names[0].name
    );
}
