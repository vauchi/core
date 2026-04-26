// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact lifecycle UniFFI bindings: soft-delete, undo, archive,
//! unarchive, and list_archived.
//!
//! These verify that the vauchi-platform surface correctly delegates to
//! the core ContactManager API and maps errors to MobileError.

use std::sync::Arc;

use tempfile::TempDir;

use vauchi_platform::VauchiPlatform;

fn setup() -> (Arc<VauchiPlatform>, TempDir) {
    let dir = TempDir::new().unwrap();
    let wb = VauchiPlatform::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
    )
    .unwrap();
    wb.create_identity("Alice".to_string()).unwrap();
    (wb, dir)
}

/// Save an imported contact and return its ID.
fn add_imported_contact(wb: &VauchiPlatform, name: &str) -> String {
    let card = vauchi_core::contact_card::ContactCard::new(name);
    let contact =
        vauchi_core::Contact::from_import(card, vauchi_core::ImportSource::VcardFile, None);
    let id = contact.id().to_string();
    wb.save_test_contact(&contact).unwrap();
    id
}

/// Save an exchanged contact and return its ID.
fn add_exchanged_contact(wb: &VauchiPlatform, name: &str) -> String {
    let card = vauchi_core::contact_card::ContactCard::new(name);
    let contact = vauchi_core::Contact::from_exchange(
        [0xAB; 32],
        card,
        vauchi_core::crypto::SymmetricKey::generate(),
    );
    let id = contact.id().to_string();
    wb.save_test_contact(&contact).unwrap();
    id
}

// === Soft-Delete (imported contacts only) ===

// @scenario: contacts_management :: Soft-delete imported contact hides from list
#[test]
fn test_soft_delete_imported_contact_hides_from_list() {
    let (wb, _dir) = setup();
    let id = add_imported_contact(&wb, "Bob");

    assert_eq!(wb.list_contacts().unwrap().len(), 1);
    wb.soft_delete_imported_contact(id.clone()).unwrap();
    assert_eq!(
        wb.list_contacts().unwrap().len(),
        0,
        "Soft-deleted contact must not appear in list_contacts"
    );
}

// @scenario: contacts_management :: Soft-delete exchanged contact fails
#[test]
fn test_soft_delete_exchanged_contact_returns_error() {
    let (wb, _dir) = setup();
    let id = add_exchanged_contact(&wb, "Carol");

    let result = wb.soft_delete_imported_contact(id);
    assert!(
        result.is_err(),
        "Soft-deleting an exchanged contact must fail"
    );
}

// @scenario: contacts_management :: Undo soft-delete restores contact
#[test]
fn test_undo_soft_delete_restores_contact_to_list() {
    let (wb, _dir) = setup();
    let id = add_imported_contact(&wb, "Dave");

    wb.soft_delete_imported_contact(id.clone()).unwrap();
    assert_eq!(wb.list_contacts().unwrap().len(), 0);

    wb.undo_delete_imported_contact(id).unwrap();
    assert_eq!(
        wb.list_contacts().unwrap().len(),
        1,
        "Undo must restore contact to visible list"
    );
}

// @scenario: contacts_management :: Hard-delete permanently removes contact
#[test]
fn test_hard_delete_permanently_removes_contact() {
    let (wb, _dir) = setup();
    let id = add_imported_contact(&wb, "Eve");

    wb.soft_delete_imported_contact(id.clone()).unwrap();
    wb.hard_delete_imported_contact(id.clone()).unwrap();

    // Even direct lookup should fail
    let contact = wb.get_contact(id).unwrap();
    assert!(
        contact.is_none(),
        "Hard-deleted contact must not be findable"
    );
}

// @scenario: contacts_management :: Hard-delete exchanged contact fails
#[test]
fn test_hard_delete_exchanged_contact_returns_error() {
    let (wb, _dir) = setup();
    let id = add_exchanged_contact(&wb, "Frank");

    let result = wb.hard_delete_imported_contact(id);
    assert!(
        result.is_err(),
        "Hard-deleting an exchanged contact must fail"
    );
}

// === Archive (exchanged contacts only) ===

// @scenario: contacts_management :: Archive exchanged contact hides from list
#[test]
fn test_archive_contact_hides_from_main_list() {
    let (wb, _dir) = setup();
    let id = add_exchanged_contact(&wb, "Grace");

    assert_eq!(wb.list_contacts().unwrap().len(), 1);
    wb.archive_contact(id).unwrap();
    assert_eq!(
        wb.list_contacts().unwrap().len(),
        0,
        "Archived contact must not appear in list_contacts"
    );
}

// @scenario: contacts_management :: Archived contacts appear in archive list
#[test]
fn test_list_archived_contacts_returns_archived() {
    let (wb, _dir) = setup();
    let id = add_exchanged_contact(&wb, "Heidi");

    assert_eq!(wb.list_archived_contacts().unwrap().len(), 0);
    wb.archive_contact(id).unwrap();

    let archived = wb.list_archived_contacts().unwrap();
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].display_name, "Heidi");
}

// @scenario: contacts_management :: Unarchive restores contact to main list
#[test]
fn test_unarchive_contact_restores_to_main_list() {
    let (wb, _dir) = setup();
    let id = add_exchanged_contact(&wb, "Ivan");

    wb.archive_contact(id.clone()).unwrap();
    assert_eq!(wb.list_contacts().unwrap().len(), 0);

    wb.unarchive_contact(id).unwrap();
    assert_eq!(
        wb.list_contacts().unwrap().len(),
        1,
        "Unarchived contact must return to list_contacts"
    );
    assert_eq!(wb.list_archived_contacts().unwrap().len(), 0);
}

// @scenario: contacts_management :: Archive imported contact fails
#[test]
fn test_archive_imported_contact_returns_error() {
    let (wb, _dir) = setup();
    let id = add_imported_contact(&wb, "Judy");

    let result = wb.archive_contact(id);
    assert!(result.is_err(), "Archiving an imported contact must fail");
}

// @scenario: contacts_management :: Nonexistent contact returns error
#[test]
fn test_lifecycle_operations_on_missing_contact_return_error() {
    let (wb, _dir) = setup();
    let fake_id = "nonexistent-id".to_string();

    assert!(wb.soft_delete_imported_contact(fake_id.clone()).is_err());
    assert!(wb.undo_delete_imported_contact(fake_id.clone()).is_err());
    assert!(wb.hard_delete_imported_contact(fake_id.clone()).is_err());
    assert!(wb.archive_contact(fake_id.clone()).is_err());
    assert!(wb.unarchive_contact(fake_id).is_err());
}

// === Contact Detail Footer Action ===
//
// Frontends call `contact_detail_footer_action_id` so the view layer
// stops branching on `MobileContact.is_imported` directly. Verifies the
// id matches what `ContactDetailEngine` would emit at the bottom of the
// detail screen.

// @internal
#[test]
fn test_contact_detail_footer_action_id_imported_returns_delete() {
    let (wb, _dir) = setup();
    let id = add_imported_contact(&wb, "Karen");

    let action_id = wb.contact_detail_footer_action_id(id).unwrap();

    assert_eq!(action_id, "delete_contact");
}

// @internal
#[test]
fn test_contact_detail_footer_action_id_exchanged_returns_archive() {
    let (wb, _dir) = setup();
    let id = add_exchanged_contact(&wb, "Liam");

    let action_id = wb.contact_detail_footer_action_id(id).unwrap();

    assert_eq!(action_id, "archive_contact");
}

// @internal
#[test]
fn test_contact_detail_footer_action_id_unknown_returns_invalid_input() {
    let (wb, _dir) = setup();

    let result = wb.contact_detail_footer_action_id("nonexistent-id".to_string());

    match result {
        Err(vauchi_platform::MobileError::InvalidInput { field, .. }) => {
            assert_eq!(
                field, "contact_id",
                "InvalidInput must name the offending field"
            );
        }
        other => panic!(
            "expected MobileError::InvalidInput {{ field: \"contact_id\", .. }}, got {:?}",
            other
        ),
    }
}
