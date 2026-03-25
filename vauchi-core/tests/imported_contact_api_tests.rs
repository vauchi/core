// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for imported contact API editing and trust guards.
//!
//! Covers:
//! - Trust operations fail for imported contacts (through the API)
//! - Editing imported contact fields succeeds
//! - Exchanged contacts cannot have fields edited via the imported-edit API
//! - Add/remove field operations on imported contacts

use vauchi_core::api::*;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::{ImportSource, VauchiError};

fn new_vauchi() -> Vauchi {
    Vauchi::in_memory().unwrap()
}

/// Creates an imported contact, saves it via the API, and returns its ID.
fn add_imported(wb: &Vauchi, name: &str) -> String {
    let card = ContactCard::new(name);
    let contact = Contact::from_import(card, ImportSource::VcardFile, None);
    let id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    id
}

/// Creates an exchanged contact, saves it via the API, and returns its ID.
fn add_exchanged(wb: &Vauchi, name: &str) -> String {
    let card = ContactCard::new(name);
    let contact = Contact::from_exchange([42u8; 32], card, SymmetricKey::generate());
    let id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    id
}

// ── Trust guard: imported contacts cannot be trusted for recovery ──────────

#[test]
fn trust_for_recovery_fails_for_imported_contact() {
    let wb = new_vauchi();
    let id = add_imported(&wb, "Alice Import");

    let result = wb.toggle_recovery_trust(&id);
    assert!(
        result.is_err(),
        "trust_for_recovery must fail for imported contacts"
    );
    match result.unwrap_err() {
        VauchiError::InvalidState(msg) => {
            assert!(
                !msg.is_empty(),
                "error message must be non-empty for trust guard"
            );
        }
        other => panic!("expected InvalidState, got {:?}", other),
    }
}

// ── Trust guard: imported contacts cannot have fingerprint verified ─────────

#[test]
fn verify_fingerprint_fails_for_imported_contact() {
    let wb = new_vauchi();
    let id = add_imported(&wb, "Bob Import");

    let result = wb.verify_contact_fingerprint(&id);
    assert!(
        result.is_err(),
        "verify_contact_fingerprint must fail for imported contacts"
    );
}

// ── Editing: update a field on an imported contact succeeds ────────────────

#[test]
fn update_imported_contact_field_succeeds() {
    let wb = new_vauchi();
    let id = add_imported(&wb, "Carol Import");

    // First add a field so we have an ID to update
    wb.add_imported_contact_field(&id, FieldType::Email, "work", "carol@old.com")
        .unwrap();

    // Retrieve the field ID
    let contact = wb.get_contact(&id).unwrap().unwrap();
    let field_id = contact
        .card()
        .fields()
        .iter()
        .find(|f| f.label() == "work")
        .expect("field must exist after add")
        .id()
        .to_string();

    // Update the field value
    wb.update_imported_contact_field(&id, &field_id, "carol@new.com")
        .unwrap();

    // Verify the update persisted
    let updated = wb.get_contact(&id).unwrap().unwrap();
    let field = updated
        .card()
        .fields()
        .iter()
        .find(|f| f.id() == field_id)
        .expect("field must still exist after update");
    assert_eq!(
        field.value(),
        "carol@new.com",
        "field value must be updated"
    );
}

// ── Editing: update a field on an exchanged contact is rejected ────────────

#[test]
fn update_imported_contact_field_rejects_exchanged() {
    let wb = new_vauchi();
    let id = add_exchanged(&wb, "Dave Exchanged");

    let result = wb.update_imported_contact_field(&id, "nonexistent-field-id", "value");
    assert!(
        result.is_err(),
        "update_imported_contact_field must reject exchanged contacts"
    );
    match result.unwrap_err() {
        VauchiError::InvalidState(msg) => {
            assert!(
                msg.contains("exchanged"),
                "error message must mention 'exchanged', got: {}",
                msg
            );
        }
        other => panic!("expected InvalidState, got {:?}", other),
    }
}

// ── Editing: add a field to an imported contact succeeds ───────────────────

#[test]
fn add_imported_contact_field_succeeds() {
    let wb = new_vauchi();
    let id = add_imported(&wb, "Eve Import");

    wb.add_imported_contact_field(&id, FieldType::Phone, "mobile", "+15551234567")
        .unwrap();

    let contact = wb.get_contact(&id).unwrap().unwrap();
    let phone = contact
        .card()
        .fields()
        .iter()
        .find(|f| f.label() == "mobile")
        .expect("added field must be present");
    assert_eq!(phone.value(), "+15551234567");
}

// ── Editing: add a field to an exchanged contact is rejected ───────────────

#[test]
fn add_imported_contact_field_rejects_exchanged() {
    let wb = new_vauchi();
    let id = add_exchanged(&wb, "Frank Exchanged");

    let result = wb.add_imported_contact_field(&id, FieldType::Phone, "mobile", "+15551234567");
    assert!(
        result.is_err(),
        "add_imported_contact_field must reject exchanged contacts"
    );
    match result.unwrap_err() {
        VauchiError::InvalidState(msg) => {
            assert!(
                msg.contains("exchanged"),
                "error must mention 'exchanged', got: {}",
                msg
            );
        }
        other => panic!("expected InvalidState, got {:?}", other),
    }
}

// ── Editing: remove a field from an imported contact succeeds ──────────────

#[test]
fn remove_imported_contact_field_succeeds() {
    let wb = new_vauchi();
    let id = add_imported(&wb, "Grace Import");

    // Add a field first
    wb.add_imported_contact_field(&id, FieldType::Email, "home", "grace@home.com")
        .unwrap();

    let contact = wb.get_contact(&id).unwrap().unwrap();
    let field_id = contact
        .card()
        .fields()
        .iter()
        .find(|f| f.label() == "home")
        .expect("field must exist after add")
        .id()
        .to_string();

    // Remove the field
    wb.remove_imported_contact_field(&id, &field_id).unwrap();

    // Verify the field is gone
    let after = wb.get_contact(&id).unwrap().unwrap();
    assert!(
        after.card().fields().iter().all(|f| f.id() != field_id),
        "removed field must not be present"
    );
}

// ── Editing: remove a field from an exchanged contact is rejected ──────────

#[test]
fn remove_imported_contact_field_rejects_exchanged() {
    let wb = new_vauchi();
    let id = add_exchanged(&wb, "Hank Exchanged");

    let result = wb.remove_imported_contact_field(&id, "any-field-id");
    assert!(
        result.is_err(),
        "remove_imported_contact_field must reject exchanged contacts"
    );
    match result.unwrap_err() {
        VauchiError::InvalidState(msg) => {
            assert!(
                msg.contains("exchanged"),
                "error must mention 'exchanged', got: {}",
                msg
            );
        }
        other => panic!("expected InvalidState, got {:?}", other),
    }
}

// ── Editing: operations on unknown contacts return ContactNotFound ──────────

#[test]
fn update_imported_contact_field_unknown_contact_returns_not_found() {
    let wb = new_vauchi();

    let result = wb.update_imported_contact_field("no-such-id", "field-id", "value");
    assert!(
        matches!(result, Err(VauchiError::ContactNotFound(_))),
        "expected ContactNotFound, got: {:?}",
        result
    );
}

#[test]
fn add_imported_contact_field_unknown_contact_returns_not_found() {
    let wb = new_vauchi();

    let result =
        wb.add_imported_contact_field("no-such-id", FieldType::Email, "label", "val@example.com");
    assert!(
        matches!(result, Err(VauchiError::ContactNotFound(_))),
        "expected ContactNotFound, got: {:?}",
        result
    );
}

#[test]
fn remove_imported_contact_field_unknown_contact_returns_not_found() {
    let wb = new_vauchi();

    let result = wb.remove_imported_contact_field("no-such-id", "field-id");
    assert!(
        matches!(result, Err(VauchiError::ContactNotFound(_))),
        "expected ContactNotFound, got: {:?}",
        result
    );
}

// ── Editing: update non-existent field returns an error ───────────────────

#[test]
fn update_imported_contact_field_missing_field_returns_error() {
    let wb = new_vauchi();
    let id = add_imported(&wb, "Ivy Import");

    let result = wb.update_imported_contact_field(&id, "no-such-field-id", "value");
    assert!(
        result.is_err(),
        "updating a non-existent field must return an error"
    );
}

// ── Editing: remove non-existent field returns an error ───────────────────

#[test]
fn remove_imported_contact_field_missing_field_returns_error() {
    let wb = new_vauchi();
    let id = add_imported(&wb, "Jack Import");

    let result = wb.remove_imported_contact_field(&id, "no-such-field-id");
    assert!(
        result.is_err(),
        "removing a non-existent field must return an error"
    );
}

// ── Import API: import_contacts_from_vcf ──────────────────────────────────

#[test]
fn import_vcf_creates_contacts() {
    let wb = new_vauchi();

    let vcf = b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice VCF\r\nEND:VCARD\r\n\
                BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob VCF\r\nEND:VCARD\r\n";

    let result = wb.import_contacts_from_vcf(vcf).unwrap();

    assert_eq!(result.imported, 2, "two contacts must be imported");
    assert_eq!(result.skipped, 0, "no contacts must be skipped");

    let contacts = wb.list_contacts().unwrap();
    let names: Vec<&str> = contacts.iter().map(|c| c.display_name()).collect();
    assert!(
        names.contains(&"Alice VCF"),
        "Alice VCF must be in contacts"
    );
    assert!(names.contains(&"Bob VCF"), "Bob VCF must be in contacts");
}

#[test]
fn import_vcf_returns_count() {
    let wb = new_vauchi();

    let vcf = b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Carol VCF\r\nEND:VCARD\r\n";

    let result = wb.import_contacts_from_vcf(vcf).unwrap();

    assert_eq!(
        result.imported, 1,
        "ImportResult.imported must match contacts created"
    );
    assert_eq!(result.skipped, 0);
    assert!(result.warnings.is_empty());
}

#[test]
fn import_empty_vcf_returns_zero() {
    let wb = new_vauchi();

    let result = wb.import_contacts_from_vcf(b"").unwrap();

    assert_eq!(result.imported, 0, "empty file must import zero contacts");
    assert_eq!(result.skipped, 0, "empty file must skip zero contacts");
    assert!(result.warnings.is_empty());
}

// ── Editing: multiple fields can be added and individually removed ─────────

#[test]
fn imported_contact_multiple_field_add_and_remove() {
    let wb = new_vauchi();
    let id = add_imported(&wb, "Karen Import");

    wb.add_imported_contact_field(&id, FieldType::Email, "work", "karen@work.com")
        .unwrap();
    wb.add_imported_contact_field(&id, FieldType::Phone, "mobile", "+15559876543")
        .unwrap();

    let contact = wb.get_contact(&id).unwrap().unwrap();
    assert_eq!(contact.card().fields().len(), 2, "two fields must be added");

    let email_field_id = contact
        .card()
        .fields()
        .iter()
        .find(|f| f.label() == "work")
        .unwrap()
        .id()
        .to_string();

    wb.remove_imported_contact_field(&id, &email_field_id)
        .unwrap();

    let after = wb.get_contact(&id).unwrap().unwrap();
    assert_eq!(after.card().fields().len(), 1, "one field must remain");
    assert_eq!(after.card().fields()[0].label(), "mobile");
}
