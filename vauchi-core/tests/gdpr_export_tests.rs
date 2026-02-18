// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! GDPR: Enhanced Export Tests
//!
//! Feature file: features/privacy_compliance.feature @export @enhanced
//! Tests for enhanced data export with devices, recovery, and consent.

mod common;

use vauchi_core::api::export_all_data;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

fn setup_storage_with_contacts() -> Storage {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    // Add a contact
    let mut card = ContactCard::new("Test Contact");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "test@example.com",
    ))
    .unwrap();
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([1u8; 32], card, shared_key);
    storage.save_contact(&contact).unwrap();

    // Save own card
    let mut own_card = ContactCard::new("My Name");
    own_card
        .add_field(ContactField::new(FieldType::Phone, "Mobile", "+1-555-0100"))
        .unwrap();
    storage.save_own_card(&own_card).unwrap();

    storage
}

#[test]
fn test_export_includes_contacts() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    assert_eq!(export.contacts.len(), 1, "Export should include contacts");
    assert_eq!(export.contacts[0].display_name, "Test Contact");
    assert!(!export.contacts[0].card_fields.is_empty());
}

#[test]
fn test_export_includes_own_card() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    assert!(export.own_card.is_some(), "Export should include own card");
}

#[test]
fn test_export_excludes_private_keys() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    // core-F-008: Parse JSON to verify structure, not just substring search.
    let json = serde_json::to_string(&export).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json)
        .expect("Export should produce valid JSON");
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

#[test]
fn test_export_includes_consent_records() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    // Add consent records
    storage
        .execute_consent_upsert("consent-1", "data_processing", true, 1000)
        .unwrap();
    storage
        .execute_consent_upsert("consent-2", "contact_sharing", true, 1001)
        .unwrap();

    let export = export_all_data(&storage).unwrap();

    assert!(
        !export.settings.consent_records.is_empty(),
        "Export should include consent records"
    );
}

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

#[test]
fn test_export_version_bumped_to_3() {
    let storage = setup_storage_with_contacts();

    let export = export_all_data(&storage).unwrap();

    assert_eq!(export.version, 3, "Export version should be 3");
    assert!(export.exported_at > 0, "Export timestamp should be set");
}

#[test]
fn test_list_audit_log_empty() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    let log = storage.list_audit_log().unwrap();
    assert!(log.is_empty(), "Empty DB should return empty audit log");
}

#[test]
fn test_list_audit_log_roundtrip() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    storage.log_audit_event("consent_granted", None).unwrap();
    storage
        .log_audit_event("data_exported", Some("full export"))
        .unwrap();

    let log = storage.list_audit_log().unwrap();
    assert_eq!(log.len(), 2);

    assert_eq!(log[0].0, "consent_granted");
    assert!(log[0].1.is_none());
    assert!(log[0].2 > 0);

    assert_eq!(log[1].0, "data_exported");
    assert_eq!(log[1].1.as_deref(), Some("full export"));
    assert!(log[1].2 > 0);
}

#[test]
fn test_list_audit_log_decrypts_details() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    storage
        .log_audit_event("sensitive_op", Some("secret details"))
        .unwrap();

    let log = storage.list_audit_log().unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(
        log[0].1.as_deref(),
        Some("secret details"),
        "Encrypted details should be decrypted on read"
    );
}

#[test]
fn test_export_includes_audit_log() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    storage
        .log_audit_event("consent_granted", Some("data_processing"))
        .unwrap();
    storage
        .log_audit_event("contact_added", Some("alice"))
        .unwrap();

    let export = export_all_data(&storage).unwrap();

    // The two events we logged should appear, plus the export event itself is logged
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
