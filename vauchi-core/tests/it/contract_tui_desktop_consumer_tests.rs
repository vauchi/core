// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Provider contract tests: core API stability for TUI and Desktop consumers (PI-04).
//!
//! TUI and Desktop use the lower-level API (Storage, Identity, ContactCard) rather
//! than the Vauchi facade. These tests verify that API surface from the
//! provider side.
//!
//! Consumers: vauchi-tui, vauchi-desktop

use vauchi_core::{ContactCard, ContactField, FieldType, Identity, Storage, SymmetricKey};

// ============================================================
// Contract: Storage::open() (Consumers: TUI, Desktop)
// ============================================================

// @internal
#[test]
fn provider_contract_storage_open_returns_result() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("provider.db");
    let key = SymmetricKey::generate();
    let storage = Storage::open(db_path.to_str().unwrap(), key);
    assert!(storage.is_ok(), "expected success");
}

// ============================================================
// Contract: Storage identity persistence (Consumers: TUI, Desktop)
// ============================================================

// @internal
#[test]
fn provider_contract_storage_save_load_identity() {
    let dir = tempfile::tempdir().unwrap();
    let key = SymmetricKey::generate();
    let storage = Storage::open(dir.path().join("p.db").to_str().unwrap(), key).unwrap();

    let data = b"identity-backup-bytes".to_vec();
    storage
        .identity()
        .save_identity(&data, "ProviderTest")
        .unwrap();

    let loaded = storage.identity().load_identity().unwrap();
    assert!(loaded.is_some(), "expected Some value");
    let (loaded_data, loaded_name) = loaded.unwrap();
    assert_eq!(loaded_data, data);
    assert_eq!(loaded_name, "ProviderTest");
}

// ============================================================
// Contract: Storage card persistence (Consumers: TUI, Desktop)
// ============================================================

// @internal
#[test]
fn provider_contract_storage_save_load_own_card_with_fields() {
    let dir = tempfile::tempdir().unwrap();
    let key = SymmetricKey::generate();
    let storage = Storage::open(dir.path().join("p.db").to_str().unwrap(), key).unwrap();

    let mut card = ContactCard::new("ProviderCard");
    card.add_field(ContactField::new(FieldType::Email, "Work", "p@test.com", 0))
        .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Mobile",
        "+1111111111",
        0,
    ))
    .unwrap();
    storage.contacts().save_own_card(&card).unwrap();

    let loaded = storage.contacts().load_own_card().unwrap().unwrap();
    assert_eq!(loaded.display_name(), "ProviderCard");
    assert_eq!(loaded.fields().len(), 2);
}

// ============================================================
// Contract: Storage contacts list (Consumers: TUI, Desktop)
// ============================================================

// @internal
#[test]
fn provider_contract_storage_list_contacts_returns_vec() {
    let dir = tempfile::tempdir().unwrap();
    let key = SymmetricKey::generate();
    let storage = Storage::open(dir.path().join("p.db").to_str().unwrap(), key).unwrap();
    let contacts = storage.contacts().list_contacts().unwrap();
    assert!(contacts.is_empty());
}

// @internal
#[test]
fn provider_contract_storage_search_contacts_returns_vec() {
    let dir = tempfile::tempdir().unwrap();
    let key = SymmetricKey::generate();
    let storage = Storage::open(dir.path().join("p.db").to_str().unwrap(), key).unwrap();
    let results = storage.contacts().search_contacts("test").unwrap();
    assert!(results.is_empty());
}

// ============================================================
// Contract: Identity creation and accessors (Consumers: TUI, Desktop)
// ============================================================

// @internal
#[test]
fn provider_contract_identity_create_has_all_fields() {
    let identity = Identity::create("ProviderIdentity", 0);
    assert_eq!(identity.display_name(), "ProviderIdentity");
    assert!(!identity.public_id().is_empty());
    assert!(!identity.device_id().is_empty());
    assert!(!identity.signing_public_key().is_empty());
}

// @internal
#[test]
fn provider_contract_identity_set_display_name() {
    let mut identity = Identity::create("Before", 0);
    identity.set_display_name("After");
    assert_eq!(identity.display_name(), "After");
}

// @internal
#[test]
fn provider_contract_identity_x3dh_keypair() {
    let identity = Identity::create("X3dhTest", 0);
    let keypair = identity.x3dh_keypair();
    assert!(!keypair.public_key().iter().all(|&b| b == 0));
}

// ============================================================
// Contract: ContactCard mutations (Consumer: Desktop)
// ============================================================

// @internal
#[test]
fn provider_contract_contact_card_remove_field() {
    let mut card = ContactCard::new("Mut");
    card.add_field(ContactField::new(FieldType::Custom, "Del", "removeme", 0))
        .unwrap();
    let fid = card.fields()[0].id().to_string();
    card.remove_field(&fid).unwrap();
    assert!(card.fields().is_empty());
}

// @internal
#[test]
fn provider_contract_contact_field_id_is_nonempty() {
    let mut card = ContactCard::new("Id");
    card.add_field(ContactField::new(FieldType::Custom, "Test", "val", 0))
        .unwrap();
    assert!(!card.fields()[0].id().is_empty());
}

// ============================================================
// Contract: SymmetricKey (Consumers: TUI, Desktop)
// ============================================================

// @internal
#[test]
fn provider_contract_symmetric_key_generate_32_bytes() {
    let key = SymmetricKey::generate();
    assert_eq!(key.as_bytes().len(), 32);
}

// @internal
#[test]
fn provider_contract_symmetric_key_from_bytes() {
    let bytes = [0x55; 32];
    let key = SymmetricKey::from_bytes(bytes);
    assert_eq!(key.as_bytes(), &bytes);
}

// ============================================================
// Contract: Exchange types exist (Consumer: Desktop)
// ============================================================

// @internal
#[test]
fn provider_contract_exchange_types_accessible() {
    use vauchi_core::exchange::{ExchangeEvent, ManualConfirmationVerifier};

    let verifier = ManualConfirmationVerifier::new();
    assert!(!verifier.is_confirmed());
    verifier.confirm();
    assert!(verifier.is_confirmed());

    assert!(matches!(ExchangeEvent::StartQR, ExchangeEvent::StartQR));
    assert!(matches!(
        ExchangeEvent::TheyScannedOurQR,
        ExchangeEvent::TheyScannedOurQR
    ));
    assert!(matches!(
        ExchangeEvent::PerformKeyAgreement,
        ExchangeEvent::PerformKeyAgreement
    ));
}
