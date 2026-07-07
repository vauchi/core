// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! GDPR: Enhanced Export Tests
//!
//! Feature file: features/privacy_compliance.feature @export @enhanced
//! Tests for enhanced data export with devices, recovery, and consent.

use vauchi_core::api::export_all_data;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

fn setup_storage_with_contacts() -> Storage {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    let mut card = ContactCard::new("Test Contact");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "test@example.com",
        0,
    ))
    .unwrap();
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([1u8; 32], card, shared_key, 0);
    storage.contacts().save_contact(&contact).unwrap();

    let mut own_card = ContactCard::new("My Name");
    own_card
        .add_field(ContactField::new(
            FieldType::Phone,
            "Mobile",
            "+1-555-0100",
            0,
        ))
        .unwrap();
    storage.contacts().save_own_card(&own_card).unwrap();

    storage
}

// @scenario: privacy_compliance :: Export includes all data types
// @internal
#[test]
fn test_export_includes_contacts() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    assert_eq!(export.contacts.len(), 1, "Export should include contacts");
    assert_eq!(export.contacts[0].display_name, "Test Contact");
    assert!(!export.contacts[0].card_fields.is_empty());
}

// @scenario: privacy_compliance :: Export includes all data types
// @internal
#[test]
fn test_export_includes_own_card() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    assert!(export.own_card.is_some(), "Export should include own card");
}

// @scenario: privacy_compliance :: Export all my data
// @internal
#[test]
fn test_export_excludes_private_keys() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    // core-F-008: Parse JSON to verify structure, not just substring search.
    let json = serde_json::to_string(&export).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("Export should produce valid JSON");
    assert!(parsed.is_object(), "Export should be a JSON object");

    // core-F-010: Check for ALL key-type patterns, not just 2.
    let sensitive_patterns = [
        "shared_key",
        "private_key",
        "signing_key",
        "signing_seed",
        "ratchet_key",
        "chain_key",
        "root_key",
        "x3dh_private",
        "ephemeral_key",
        "master_seed",
    ];

    let json_lower = json.to_lowercase();
    for pattern in &sensitive_patterns {
        assert!(
            !json_lower.contains(pattern),
            "Export must not contain '{}' — found in: {}",
            pattern,
            &json[..json.len().min(500)]
        );
    }
}

// @scenario: privacy_compliance :: Export includes all data types
// @internal
#[test]
fn test_export_includes_consent_records() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    storage
        .consent()
        .execute_consent_upsert("consent-1", "data_processing", true, 1000)
        .unwrap();
    storage
        .consent()
        .execute_consent_upsert("consent-2", "contact_sharing", true, 1001)
        .unwrap();

    let export = export_all_data(&storage).unwrap();

    assert!(
        !export.settings.consent_records.is_empty(),
        "Export should include consent records"
    );
}

// @scenario: privacy_compliance :: Export includes device list and recovery config
// @internal
#[test]
fn test_export_includes_devices() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    // devices field should be present (may be empty if no devices registered)
    assert!(
        export.devices.is_some(),
        "Export should include devices field"
    );
}

// @scenario: privacy_compliance :: Export includes device list and recovery config
// @internal
#[test]
fn test_export_includes_recovery_config() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    // recovery_config field should be present
    assert!(
        export.recovery_config.is_some(),
        "Export should include recovery config field"
    );
}

// @scenario: privacy_compliance :: Export all my data
// @internal
#[test]
fn test_export_version_bumped_to_3() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    assert_eq!(export.version, 3, "Export version should be 3");
    assert!(export.exported_at > 0, "Export timestamp should be set");
}

// @internal
#[test]
fn test_list_audit_log_empty() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    let log = storage.consent().list_audit_log().unwrap();
    assert!(log.is_empty(), "Empty DB should return empty audit log");
}

// @internal
#[test]
fn test_list_audit_log_roundtrip() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    storage
        .consent()
        .log_audit_event("consent_granted", None)
        .unwrap();
    storage
        .consent()
        .log_audit_event("data_exported", Some("full export"))
        .unwrap();

    let log = storage.consent().list_audit_log().unwrap();
    assert_eq!(log.len(), 2);

    assert_eq!(log[0].0, "consent_granted");
    assert!(log[0].1.is_none());
    assert!(log[0].2 > 0);

    assert_eq!(log[1].0, "data_exported");
    assert_eq!(log[1].1.as_deref(), Some("full export"));
    assert!(log[1].2 > 0);
}

// @internal
#[test]
fn test_list_audit_log_decrypts_details() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    storage
        .consent()
        .log_audit_event("sensitive_op", Some("secret details"))
        .unwrap();

    let log = storage.consent().list_audit_log().unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(
        log[0].1.as_deref(),
        Some("secret details"),
        "Encrypted details should be decrypted on read"
    );
}

// @scenario: privacy_compliance :: Export includes all data types
// @internal
#[test]
fn test_export_includes_audit_log() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    storage
        .consent()
        .log_audit_event("consent_granted", Some("data_processing"))
        .unwrap();
    storage
        .consent()
        .log_audit_event("contact_added", Some("alice"))
        .unwrap();

    let export = export_all_data(&storage).unwrap();

    // but after the snapshot, so only our 2 events are in the export.
    assert_eq!(
        export.audit_log.len(),
        2,
        "Export should include the 2 audit events"
    );

    assert_eq!(export.audit_log[0]["event_type"], "consent_granted");
    assert_eq!(export.audit_log[0]["details"], "data_processing");
    assert_eq!(export.audit_log[1]["event_type"], "contact_added");
    assert_eq!(export.audit_log[1]["details"], "alice");
}

// @scenario: privacy_compliance :: Encrypted GDPR export roundtrip
// @internal
#[test]
fn test_encrypted_export_roundtrip() {
    use vauchi_core::api::{export_encrypted, import_encrypted};

    let storage = setup_storage_with_contacts();
    let password = "hunter2-test-password";

    let encrypted = export_encrypted(&storage, password).unwrap();

    assert_eq!(encrypted[0], vauchi_core::api::GDPR_EXPORT_VERSION);
    assert!(encrypted.len() > 1 + vauchi_core::api::GDPR_SALT_LEN);

    let recovered = import_encrypted(&encrypted, password).unwrap();
    assert_eq!(recovered.version, 3);
    assert_eq!(recovered.contacts.len(), 1);
    assert_eq!(recovered.contacts[0].display_name, "Test Contact");
}

// @scenario: privacy_compliance :: Wrong password fails decryption
// @internal
#[test]
fn test_encrypted_export_wrong_password_fails() {
    use vauchi_core::api::{export_encrypted, import_encrypted};

    let storage = setup_storage_with_contacts();
    let encrypted = export_encrypted(&storage, "correct").unwrap();

    let result = import_encrypted(&encrypted, "wrong");
    assert!(result.is_err(), "Wrong password must fail decryption");
}
