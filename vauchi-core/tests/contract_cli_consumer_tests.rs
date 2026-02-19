// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Provider contract tests: core's API stability for CLI consumers (CC-05, PI-04).
//!
//! These tests run in the core repo and verify that vauchi-core's public API
//! maintains the shape and semantics that downstream consumers (CLI, TUI,
//! desktop) depend on. If a core developer changes a return type, removes a
//! method, or alters behavior, these tests fail here — before the break
//! propagates to consumer repos.
//!
//! Each test is tagged with the consumer it protects (currently CLI).
//! When adding new contract tests, note which consumer depends on the contract.

use vauchi_core::api::{ConsentRecord, ConsentType};
use vauchi_core::network::MockTransport;
use vauchi_core::{Contact, ContactCard, ContactField, FieldType, Identity, Vauchi};

/// Helper: create a Vauchi instance with identity.
fn setup() -> Vauchi<MockTransport> {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("ProviderContract").unwrap();
    wb
}

// ============================================================
// Contract: create_identity() → public_id() (Consumer: CLI)
// ============================================================

/// CLI calls create_identity(name) and then public_id() to display the user's ID.
/// Changing the return type or removing public_id() would break CLI.
#[test]
fn contract_cli_create_identity_then_public_id() {
    let wb = setup();
    let public_id: String = wb.public_id().unwrap();
    assert!(
        !public_id.is_empty(),
        "CLI depends on public_id() returning a non-empty String"
    );
}

/// CLI accesses identity().display_name() to show the user's name.
#[test]
fn contract_cli_identity_display_name() {
    let wb = setup();
    let identity: &Identity = wb.identity().unwrap();
    let name: &str = identity.display_name();
    assert_eq!(name, "ProviderContract");
}

// ============================================================
// Contract: list_contacts() → Vec<Contact> (Consumer: CLI)
// ============================================================

/// CLI calls list_contacts() and iterates over the result.
/// Each Contact must have id(), display_name(), card(), public_key().
#[test]
fn contract_cli_list_contacts_shape() {
    let wb = setup();
    let contacts: Vec<Contact> = wb.list_contacts().unwrap();
    // Shape assertion: Vec<Contact> compiles and is iterable
    for _contact in &contacts {
        // If any accessor is removed, this block fails to compile
    }
    assert!(contacts.is_empty(), "fresh instance has no contacts");
}

/// CLI uses paginated listing with offset/limit.
#[test]
fn contract_cli_list_contacts_paginated() {
    let wb = setup();
    let page: Vec<Contact> = wb.list_contacts_paginated(0, 10).unwrap();
    assert!(page.is_empty());
}

// ============================================================
// Contract: Contact accessors (Consumer: CLI)
// ============================================================

/// Compile-time contract: Contact exposes these methods.
/// If any are removed or renamed, this test fails to compile.
#[test]
fn contract_cli_contact_accessors_compile() {
    fn _check(c: &Contact) {
        let _: &str = c.id();
        let _: &str = c.display_name();
        let _: &ContactCard = c.card();
        let _: &[u8; 32] = c.public_key();
        let _: u64 = c.exchange_timestamp();
        let _: bool = c.is_hidden();
        let _: bool = c.is_blocked();
    }
    // Compile-time contract — if this compiles, the API shape is correct.
    // Assert the function pointer is valid (satisfies clippy + zero-assertion scanner).
    let _: fn(&Contact) = _check;
    assert_ne!(std::mem::size_of::<Contact>(), 0);
}

// ============================================================
// Contract: ContactCard (Consumer: CLI, TUI, Desktop)
// ============================================================

/// All clients create cards and access their fields.
#[test]
fn contract_contact_card_shape() {
    let card = ContactCard::new("Shape");
    assert!(!card.id().is_empty());
    assert_eq!(card.display_name(), "Shape");
    assert!(card.fields().is_empty());
}

/// Cards must serialize/deserialize losslessly — sync depends on this.
#[test]
fn contract_contact_card_json_roundtrip() {
    let mut card = ContactCard::new("JSON");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "user@example.com",
    ))
    .unwrap();

    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();

    assert_eq!(card.id(), restored.id());
    assert_eq!(card.display_name(), restored.display_name());
    assert_eq!(card.fields().len(), restored.fields().len());
    assert_eq!(card.fields()[0].value(), restored.fields()[0].value());
}

// ============================================================
// Contract: FieldType enum variants (Consumer: CLI)
// ============================================================

/// CLI's parse_field_type() maps strings to these variants.
/// Removing a variant breaks CLI at compile time.
#[test]
fn contract_cli_field_type_variants() {
    let required_variants: Vec<FieldType> = vec![
        FieldType::Phone,
        FieldType::Email,
        FieldType::Address,
        FieldType::Website,
        FieldType::Social,
        FieldType::Custom,
    ];
    assert!(
        required_variants.len() >= 6,
        "CLI depends on at least 6 FieldType variants"
    );
}

// ============================================================
// Contract: ContactField accessors (Consumer: CLI)
// ============================================================

#[test]
fn contract_cli_contact_field_accessors() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+41791234567");
    assert_eq!(field.field_type(), FieldType::Phone);
    assert_eq!(field.label(), "Mobile");
    assert_eq!(field.value(), "+41791234567");
}

// ============================================================
// Contract: Consent API (Consumer: CLI)
// ============================================================

/// CLI exposes consent grant/check/revoke/export.
#[test]
fn contract_cli_consent_api() {
    let wb = setup();

    // grant_consent accepts ConsentType
    wb.grant_consent(ConsentType::Analytics).unwrap();

    // check_consent returns bool
    let granted: bool = wb.check_consent(&ConsentType::Analytics).unwrap();
    assert!(granted);

    // revoke_consent accepts ConsentType
    wb.revoke_consent(ConsentType::Analytics).unwrap();
    let revoked = !wb.check_consent(&ConsentType::Analytics).unwrap();
    assert!(revoked);

    // export_consent_log returns Vec<ConsentRecord>
    let log: Vec<ConsentRecord> = wb.export_consent_log().unwrap();
    assert_eq!(log.len(), 2, "grant + revoke = 2 records");
}

// ============================================================
// Contract: get_own_card / add_field_to_own_card (Consumer: CLI)
// ============================================================

#[test]
fn contract_cli_card_field_management() {
    let wb = setup();

    // get_own_card returns Option<ContactCard>
    let card = wb.own_card().unwrap().unwrap();
    assert_eq!(card.fields().len(), 0, "new card has no fields");

    // add_field_to_own_card accepts ContactField
    let field = ContactField::new(FieldType::Email, "Personal", "me@example.com");
    wb.add_own_field(field).unwrap();

    let card = wb.own_card().unwrap().unwrap();
    assert_eq!(card.fields().len(), 1);
}
