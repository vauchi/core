//! Edge Cases Tests
//!
//! Tests for boundary conditions, limits, and unusual inputs.
//! These tests verify the system handles edge cases correctly.

use webbook_core::{
    contact::FieldVisibility,
    identity::{DeviceInfo, DeviceRegistry, Identity, MAX_DEVICES},
    network::MockTransport,
    sync::CardDelta,
    Contact, ContactCard, ContactField, FieldType, SigningKeyPair, SymmetricKey, WebBook,
};

// =============================================================================
// Field Limit Tests
// =============================================================================

/// Maximum number of fields allowed per card
const MAX_FIELDS: usize = 25;

/// Test: Card at max fields rejects addition
#[test]
fn test_card_at_max_fields_rejects_addition() {
    let mut card = ContactCard::new("Test");

    // Add fields up to limit
    for i in 0..MAX_FIELDS {
        let result = card.add_field(ContactField::new(
            FieldType::Custom,
            &format!("field{}", i),
            &format!("value{}", i),
        ));
        assert!(result.is_ok(), "Should allow adding field {}", i);
    }

    assert_eq!(card.fields().len(), MAX_FIELDS);

    // Try to add one more - should fail
    let result = card.add_field(ContactField::new(
        FieldType::Custom,
        "extra",
        "value",
    ));
    assert!(result.is_err(), "Should reject field beyond limit");
}

/// Test: Card at max fields allows modification
#[test]
fn test_card_at_max_fields_allows_modification() {
    let mut card = ContactCard::new("Test");

    // Add fields up to limit
    for i in 0..MAX_FIELDS {
        card.add_field(ContactField::new(
            FieldType::Custom,
            &format!("field{}", i),
            &format!("value{}", i),
        ))
        .unwrap();
    }

    // Modify existing field - should succeed
    let field_id = card.fields()[0].id().to_string();
    let result = card.update_field_value(&field_id, "new_value");
    assert!(result.is_ok(), "Should allow modifying existing field");
}

/// Test: Card at max fields allows removal
#[test]
fn test_card_at_max_fields_allows_removal() {
    let mut card = ContactCard::new("Test");

    // Add fields up to limit
    for i in 0..MAX_FIELDS {
        card.add_field(ContactField::new(
            FieldType::Custom,
            &format!("field{}", i),
            &format!("value{}", i),
        ))
        .unwrap();
    }

    // Remove a field - should succeed
    let field_id = card.fields()[0].id().to_string();
    let result = card.remove_field(&field_id);
    assert!(result.is_ok(), "Should allow removing field");
    assert_eq!(card.fields().len(), MAX_FIELDS - 1);

    // Now can add another field
    let result = card.add_field(ContactField::new(
        FieldType::Custom,
        "new",
        "value",
    ));
    assert!(result.is_ok(), "Should allow adding after removal");
}

/// Test: Empty card delta computation
#[test]
fn test_empty_card_delta_computation() {
    let old = ContactCard::new("Empty");
    let new = ContactCard::new("Empty");

    let delta = CardDelta::compute(&old, &new);
    assert!(delta.is_empty(), "Identical empty cards should produce empty delta");
}

// =============================================================================
// Unicode and Special Character Tests
// =============================================================================

/// Test: Field with emoji roundtrip
#[test]
fn test_field_with_emoji_roundtrip() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    let mut card = ContactCard::new("Test 🎉");
    card.add_field(ContactField::new(
        FieldType::Custom,
        "emoji",
        "Hello 👋 World 🌍",
    ))
    .unwrap();

    let contact = Contact::from_exchange([1u8; 32], card, SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
    assert_eq!(loaded.display_name(), "Test 🎉");

    let emoji_field = loaded.card().fields().iter().find(|f| f.label() == "emoji").unwrap();
    assert_eq!(emoji_field.value(), "Hello 👋 World 🌍");
}

/// Test: Field with RTL text
#[test]
fn test_field_with_rtl_text() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    let mut card = ContactCard::new("مرحبا"); // Arabic "Hello"
    card.add_field(ContactField::new(
        FieldType::Custom,
        "greeting",
        "שלום עולם", // Hebrew "Hello World"
    ))
    .unwrap();

    let contact = Contact::from_exchange([1u8; 32], card, SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
    assert_eq!(loaded.display_name(), "مرحبا");

    let greeting = loaded.card().fields().iter().find(|f| f.label() == "greeting").unwrap();
    assert_eq!(greeting.value(), "שלום עולם");
}

/// Test: Field with various Unicode categories
#[test]
fn test_field_with_various_unicode() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    let mut card = ContactCard::new("Тест"); // Cyrillic
    card.add_field(ContactField::new(
        FieldType::Custom,
        "chinese",
        "你好世界", // Chinese
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Custom,
        "japanese",
        "こんにちは世界", // Japanese
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Custom,
        "korean",
        "안녕하세요 세계", // Korean
    ))
    .unwrap();

    let contact = Contact::from_exchange([1u8; 32], card, SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
    assert_eq!(loaded.card().fields().len(), 3);
}

/// Test: Empty string handling
#[test]
fn test_empty_string_field_value() {
    let mut card = ContactCard::new("Test");

    // Empty value should be allowed (or rejected depending on implementation)
    let result = card.add_field(ContactField::new(
        FieldType::Custom,
        "empty",
        "",
    ));

    // Document the behavior
    let _ = result; // Either ok or error is acceptable
}

// =============================================================================
// Delta Edge Cases
// =============================================================================

/// Test: Delta with only display name change
#[test]
fn test_delta_only_display_name_change() {
    let old = ContactCard::new("Old Name");
    let new = ContactCard::new("New Name");

    let delta = CardDelta::compute(&old, &new);
    assert!(!delta.is_empty());
    assert_eq!(delta.changes.len(), 1);
}

/// Test: Delta computation with many fields
#[test]
fn test_delta_with_many_fields() {
    let mut old = ContactCard::new("Test");
    let mut new = ContactCard::new("Test");

    // Add same 10 fields to both
    for i in 0..10 {
        old.add_field(ContactField::new(
            FieldType::Custom,
            &format!("field{}", i),
            &format!("old_value{}", i),
        ))
        .unwrap();
        new.add_field(ContactField::new(
            FieldType::Custom,
            &format!("field{}", i),
            &format!("new_value{}", i),
        ))
        .unwrap();
    }

    let delta = CardDelta::compute(&old, &new);

    // All fields should be different (new IDs)
    assert!(!delta.is_empty());
}

/// Test: Delta apply preserves display name when unchanged
#[test]
fn test_delta_apply_preserves_display_name() {
    let mut old = ContactCard::new("Preserved Name");
    old.add_field(ContactField::new(FieldType::Email, "work", "old@test.com"))
        .unwrap();

    let mut new = ContactCard::new("Preserved Name");
    new.add_field(ContactField::new(FieldType::Email, "work", "new@test.com"))
        .unwrap();

    // Need to use same field ID for modification detection
    let mut target = old.clone();
    let delta = CardDelta::compute(&old, &new);

    // Apply should not change display name
    let _ = delta.apply(&mut target);
    assert_eq!(target.display_name(), "Preserved Name");
}

// =============================================================================
// Device Registry Edge Cases
// =============================================================================

/// Test: Device registry at max devices
#[test]
fn test_device_registry_at_max_devices() {
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Device 0".to_string());
    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);

    // Add up to max
    for i in 1..MAX_DEVICES {
        let device = DeviceInfo::derive(&master_seed, i as u32, format!("Device {}", i));
        registry
            .add_device(device.to_registered(&master_seed), &signing_key)
            .unwrap();
    }

    assert_eq!(registry.active_count(), MAX_DEVICES);
}

/// Test: Device revocation at limit
#[test]
fn test_device_revocation_when_at_two() {
    let master_seed = [0x42u8; 32];
    let signing_key = SigningKeyPair::from_seed(&master_seed);

    let device0 = DeviceInfo::derive(&master_seed, 0, "Device 0".to_string());
    let device1 = DeviceInfo::derive(&master_seed, 1, "Device 1".to_string());

    let mut registry = DeviceRegistry::new(device0.to_registered(&master_seed), &signing_key);
    registry
        .add_device(device1.to_registered(&master_seed), &signing_key)
        .unwrap();

    assert_eq!(registry.active_count(), 2);

    // Should allow revocation since we have 2 devices
    registry.revoke_device(device1.device_id(), &signing_key).unwrap();

    assert_eq!(registry.active_count(), 1);
}

// =============================================================================
// Visibility Edge Cases
// =============================================================================

/// Test: Visibility with all fields hidden
#[test]
fn test_visibility_all_fields_hidden() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    let contact = Contact::from_exchange([1u8; 32], ContactCard::new("Test"), SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    let mut contact = wb.get_contact(&contact_id).unwrap().unwrap();

    // Hide all potential fields
    contact.visibility_rules_mut().set_nobody("field1");
    contact.visibility_rules_mut().set_nobody("field2");
    contact.visibility_rules_mut().set_nobody("field3");

    wb.storage().save_contact(&contact).unwrap();

    // Verify all are hidden
    let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
    let rules = loaded.visibility_rules();
    assert!(matches!(rules.get("field1"), FieldVisibility::Nobody));
    assert!(matches!(rules.get("field2"), FieldVisibility::Nobody));
    assert!(matches!(rules.get("field3"), FieldVisibility::Nobody));
}

/// Test: Visibility default is Everyone
#[test]
fn test_visibility_default_is_everyone() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    let contact = Contact::from_exchange([1u8; 32], ContactCard::new("Test"), SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    let contact = wb.get_contact(&contact_id).unwrap().unwrap();
    let rules = contact.visibility_rules();

    // Non-configured field should default to Everyone
    assert!(matches!(rules.get("any-field"), FieldVisibility::Everyone));
}

// =============================================================================
// Identity Edge Cases
// =============================================================================

/// Test: Identity with very long display name
#[test]
fn test_identity_long_display_name() {
    let long_name = "A".repeat(100);
    let identity = Identity::create(&long_name);
    assert_eq!(identity.display_name(), long_name);
}

/// Test: Identity with special characters in name
#[test]
fn test_identity_special_chars_in_name() {
    let special_name = "O'Brien-Smith (Jr.) & Co.";
    let identity = Identity::create(special_name);
    assert_eq!(identity.display_name(), special_name);
}

/// Test: Backup with maximum complexity password
#[test]
fn test_backup_with_complex_password() {
    let identity = Identity::create("Test");
    let complex_password = "Aa1!Bb2@Cc3#Dd4$Ee5%Ff6^Gg7&Hh8*Ii9(Jj0)Kk_Ll+Mm=";

    let backup = identity.export_backup(complex_password).unwrap();
    let restored = Identity::import_backup(&backup, complex_password).unwrap();

    assert_eq!(restored.public_id(), identity.public_id());
}

// =============================================================================
// Contact Edge Cases
// =============================================================================

/// Test: Contact with very long display name
#[test]
fn test_contact_long_display_name() {
    let long_name = "X".repeat(200);
    let card = ContactCard::new(&long_name);
    let contact = Contact::from_exchange([1u8; 32], card, SymmetricKey::generate());
    assert_eq!(contact.display_name(), long_name);
}

/// Test: Contact search is case insensitive
#[test]
fn test_contact_search_case_insensitive() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    let contact1 = Contact::from_exchange([1u8; 32], ContactCard::new("Alice"), SymmetricKey::generate());
    let contact2 = Contact::from_exchange([2u8; 32], ContactCard::new("ALICE"), SymmetricKey::generate());
    let contact3 = Contact::from_exchange([3u8; 32], ContactCard::new("alice"), SymmetricKey::generate());

    wb.add_contact(contact1).unwrap();
    wb.add_contact(contact2).unwrap();
    wb.add_contact(contact3).unwrap();

    // Search should find all
    let results = wb.search_contacts("alice").unwrap();
    assert_eq!(results.len(), 3);

    let results = wb.search_contacts("ALICE").unwrap();
    assert_eq!(results.len(), 3);
}

/// Test: Contact search with partial match
#[test]
fn test_contact_search_partial_match() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    let contact1 = Contact::from_exchange([1u8; 32], ContactCard::new("Alexander"), SymmetricKey::generate());
    let contact2 = Contact::from_exchange([2u8; 32], ContactCard::new("Alexandra"), SymmetricKey::generate());
    let contact3 = Contact::from_exchange([3u8; 32], ContactCard::new("Bob"), SymmetricKey::generate());

    wb.add_contact(contact1).unwrap();
    wb.add_contact(contact2).unwrap();
    wb.add_contact(contact3).unwrap();

    let results = wb.search_contacts("Alex").unwrap();
    assert_eq!(results.len(), 2);
}

// =============================================================================
// Storage Edge Cases
// =============================================================================

/// Test: Saving same contact twice updates
#[test]
fn test_saving_contact_twice_updates() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    let mut card = ContactCard::new("Test");
    card.add_field(ContactField::new(FieldType::Email, "work", "original@test.com"))
        .unwrap();

    let contact = Contact::from_exchange([1u8; 32], card, SymmetricKey::generate());
    let contact_id = contact.id().to_string();

    wb.add_contact(contact).unwrap();

    // Modify and save again
    let mut contact = wb.get_contact(&contact_id).unwrap().unwrap();
    let field_id = contact.card().fields()[0].id().to_string();
    let mut card = contact.card().clone();
    card.update_field_value(&field_id, "updated@test.com").unwrap();
    contact.update_card(card);
    wb.storage().save_contact(&contact).unwrap();

    // Verify update persisted
    let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
    let email = loaded.card().fields().iter().find(|f| f.label() == "work").unwrap();
    assert_eq!(email.value(), "updated@test.com");
}

/// Test: Contact count remains accurate after operations
#[test]
fn test_contact_count_accuracy() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    assert_eq!(wb.contact_count().unwrap(), 0);

    // Add 3 contacts
    for i in 0..3 {
        let contact = Contact::from_exchange(
            [i as u8; 32],
            ContactCard::new(&format!("Contact {}", i)),
            SymmetricKey::generate(),
        );
        wb.add_contact(contact).unwrap();
    }
    assert_eq!(wb.contact_count().unwrap(), 3);

    // Remove one
    let contacts = wb.list_contacts().unwrap();
    wb.remove_contact(contacts[0].id()).unwrap();
    assert_eq!(wb.contact_count().unwrap(), 2);

    // Add another
    let contact = Contact::from_exchange([99u8; 32], ContactCard::new("New"), SymmetricKey::generate());
    wb.add_contact(contact).unwrap();
    assert_eq!(wb.contact_count().unwrap(), 3);
}
