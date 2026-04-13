// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact name variant operations (internal API).
//!
//! @scenario: contacts_management.feature - Per-group name variant selection

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
fn test_upsert_and_list_name_variant() {
    let (wb, cid) = setup_with_contact();
    wb.upsert_contact_name_variant(&cid, "Work", "Dr. Bob", None)
        .unwrap();
    let variants = wb.list_contact_name_variants(&cid).unwrap();
    assert_eq!(variants.len(), 1);
    assert_eq!(variants[0].source_label, "Work");
    assert_eq!(variants[0].name, "Dr. Bob");
    assert!(!variants[0].has_avatar);
}

#[test]
fn test_upsert_updates_existing_variant() {
    let (wb, cid) = setup_with_contact();
    wb.upsert_contact_name_variant(&cid, "Work", "Dr. Bob", None)
        .unwrap();
    wb.upsert_contact_name_variant(&cid, "Work", "Professor Bob", None)
        .unwrap();
    let variants = wb.list_contact_name_variants(&cid).unwrap();
    assert_eq!(variants.len(), 1, "Upsert must not create duplicate");
    assert_eq!(variants[0].name, "Professor Bob");
}

#[test]
fn test_multiple_variants() {
    let (wb, cid) = setup_with_contact();
    wb.upsert_contact_name_variant(&cid, "", "Bob Smith", None)
        .unwrap();
    wb.upsert_contact_name_variant(&cid, "Work", "Dr. Smith", None)
        .unwrap();
    wb.upsert_contact_name_variant(&cid, "Family", "Bobby", None)
        .unwrap();
    let variants = wb.list_contact_name_variants(&cid).unwrap();
    assert_eq!(variants.len(), 3);
}

#[test]
fn test_list_variants_empty_when_none() {
    let (wb, cid) = setup_with_contact();
    let variants = wb.list_contact_name_variants(&cid).unwrap();
    assert!(variants.is_empty());
}

#[test]
fn test_variant_with_avatar() {
    let (wb, cid) = setup_with_contact();
    let avatar = vec![1, 2, 3, 4];
    wb.upsert_contact_name_variant(&cid, "Work", "Dr. Bob", Some(&avatar))
        .unwrap();
    let variants = wb.list_contact_name_variants(&cid).unwrap();
    assert!(
        variants[0].has_avatar,
        "Variant with avatar data must report has_avatar=true"
    );
}

#[test]
fn test_default_variant_uses_empty_label() {
    let (wb, cid) = setup_with_contact();
    wb.upsert_contact_name_variant(&cid, "", "Default Bob", None)
        .unwrap();
    let variants = wb.list_contact_name_variants(&cid).unwrap();
    assert_eq!(variants[0].source_label, "");
    assert_eq!(variants[0].name, "Default Bob");
}
