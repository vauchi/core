//! Snapshot Tests for Serialization
//!
//! These tests verify that serialization output matches expected snapshots.
//! Unlike protocol compatibility tests (which test deserialization of golden
//! fixtures), these tests verify that our code produces the expected output.
//!
//! If serialization changes, these tests will fail, alerting us to review
//! whether the change is intentional.

// =============================================================================
// CONTACT CARD SNAPSHOTS
// =============================================================================

#[test]
fn test_contact_card_serialization_snapshot() {
use webbook_core::ContactCard;

    // Create a card with known ID (normally random)
    let card_json = r#"{"id":"snapshot-card-001","display_name":"Snapshot User","fields":[{"id":"field-001","field_type":"Email","label":"Work","value":"work@example.com"},{"id":"field-002","field_type":"Phone","label":"Mobile","value":"+1234567890"}]}"#;

    // Deserialize and re-serialize to verify format stability
    let card: ContactCard = serde_json::from_str(card_json).unwrap();
    let reserialized = serde_json::to_string(&card).unwrap();

    // Parse both as JSON values to compare (ignores whitespace differences)
    let original: serde_json::Value = serde_json::from_str(card_json).unwrap();
    let output: serde_json::Value = serde_json::from_str(&reserialized).unwrap();

    assert_eq!(original, output, "ContactCard serialization changed");
}

#[test]
fn test_field_type_serialization_snapshot() {
use webbook_core::FieldType;

    // Each variant should serialize to its exact name
    assert_eq!(
        serde_json::to_string(&FieldType::Phone).unwrap(),
        "\"Phone\""
    );
    assert_eq!(
        serde_json::to_string(&FieldType::Email).unwrap(),
        "\"Email\""
    );
    assert_eq!(
        serde_json::to_string(&FieldType::Social).unwrap(),
        "\"Social\""
    );
    assert_eq!(
        serde_json::to_string(&FieldType::Address).unwrap(),
        "\"Address\""
    );
    assert_eq!(
        serde_json::to_string(&FieldType::Website).unwrap(),
        "\"Website\""
    );
    assert_eq!(
        serde_json::to_string(&FieldType::Custom).unwrap(),
        "\"Custom\""
    );
}

#[test]
fn test_contact_field_serialization_snapshot() {
use webbook_core::ContactField;

    let field_json =
        r#"{"id":"test-field","field_type":"Email","label":"Personal","value":"me@example.com"}"#;

    let field: ContactField = serde_json::from_str(field_json).unwrap();
    let reserialized = serde_json::to_string(&field).unwrap();

    let original: serde_json::Value = serde_json::from_str(field_json).unwrap();
    let output: serde_json::Value = serde_json::from_str(&reserialized).unwrap();

    assert_eq!(original, output, "ContactField serialization changed");
}

// =============================================================================
// CARD DELTA SNAPSHOTS
// =============================================================================

#[test]
fn test_field_change_added_serialization_snapshot() {
use webbook_core::sync::FieldChange;
use webbook_core::ContactField;

    let field_json =
        r#"{"id":"new-field","field_type":"Phone","label":"Home","value":"+9876543210"}"#;
    let field: ContactField = serde_json::from_str(field_json).unwrap();

    let change = FieldChange::Added { field };
    let serialized = serde_json::to_string(&change).unwrap();

    // Verify the enum variant name is "Added" and structure is correct
    assert!(serialized.contains("\"Added\""));
    assert!(serialized.contains("\"field\""));
    assert!(serialized.contains("\"new-field\""));
}

#[test]
fn test_field_change_modified_serialization_snapshot() {
use webbook_core::sync::FieldChange;

    let change = FieldChange::Modified {
        field_id: "existing-field".to_string(),
        new_value: "updated-value".to_string(),
    };

    let serialized = serde_json::to_string(&change).unwrap();

    // Expected format
    let expected = r#"{"Modified":{"field_id":"existing-field","new_value":"updated-value"}}"#;
    assert_eq!(serialized, expected);
}

#[test]
fn test_field_change_removed_serialization_snapshot() {
use webbook_core::sync::FieldChange;

    let change = FieldChange::Removed {
        field_id: "removed-field".to_string(),
    };

    let serialized = serde_json::to_string(&change).unwrap();

    let expected = r#"{"Removed":{"field_id":"removed-field"}}"#;
    assert_eq!(serialized, expected);
}

#[test]
fn test_field_change_display_name_changed_serialization_snapshot() {
use webbook_core::sync::FieldChange;

    let change = FieldChange::DisplayNameChanged {
        new_name: "New Display Name".to_string(),
    };

    let serialized = serde_json::to_string(&change).unwrap();

    let expected = r#"{"DisplayNameChanged":{"new_name":"New Display Name"}}"#;
    assert_eq!(serialized, expected);
}

// =============================================================================
// SYNC ITEM SNAPSHOTS
// =============================================================================

#[test]
fn test_sync_item_card_updated_serialization_snapshot() {
use webbook_core::sync::SyncItem;

    let item = SyncItem::CardUpdated {
        field_label: "email".to_string(),
        new_value: "new@example.com".to_string(),
        timestamp: 1700000000,
    };

    let serialized = serde_json::to_string(&item).unwrap();

    let expected = r#"{"CardUpdated":{"field_label":"email","new_value":"new@example.com","timestamp":1700000000}}"#;
    assert_eq!(serialized, expected);
}

#[test]
fn test_sync_item_contact_removed_serialization_snapshot() {
use webbook_core::sync::SyncItem;

    let item = SyncItem::ContactRemoved {
        contact_id: "contact-to-remove".to_string(),
        timestamp: 1700000100,
    };

    let serialized = serde_json::to_string(&item).unwrap();

    let expected =
        r#"{"ContactRemoved":{"contact_id":"contact-to-remove","timestamp":1700000100}}"#;
    assert_eq!(serialized, expected);
}

#[test]
fn test_sync_item_visibility_changed_serialization_snapshot() {
use webbook_core::sync::SyncItem;

    let item = SyncItem::VisibilityChanged {
        contact_id: "contact-123".to_string(),
        field_label: "phone".to_string(),
        is_visible: false,
        timestamp: 1700000200,
    };

    let serialized = serde_json::to_string(&item).unwrap();

    let expected = r#"{"VisibilityChanged":{"contact_id":"contact-123","field_label":"phone","is_visible":false,"timestamp":1700000200}}"#;
    assert_eq!(serialized, expected);
}

// =============================================================================
// DEVICE SNAPSHOTS
// =============================================================================

#[test]
fn test_registered_device_serialization_snapshot() {
use webbook_core::identity::RegisteredDevice;

    let device = RegisteredDevice {
        device_id: [1u8; 32],
        exchange_public_key: [2u8; 32],
        device_name: "Test Phone".to_string(),
        created_at: 1700000000,
        revoked: false,
        revoked_at: None,
    };

    let serialized = serde_json::to_string(&device).unwrap();

    // Verify key fields are present
    assert!(serialized.contains("\"device_id\""));
    assert!(serialized.contains("\"exchange_public_key\""));
    assert!(serialized.contains("\"device_name\":\"Test Phone\""));
    assert!(serialized.contains("\"created_at\":1700000000"));
    assert!(serialized.contains("\"revoked\":false"));
    assert!(serialized.contains("\"revoked_at\":null"));

    // Round-trip test
    let reparsed: RegisteredDevice = serde_json::from_str(&serialized).unwrap();
    assert_eq!(reparsed.device_name, "Test Phone");
    assert_eq!(reparsed.created_at, 1700000000);
}

#[test]
fn test_registered_device_revoked_serialization_snapshot() {
use webbook_core::identity::RegisteredDevice;

    let device = RegisteredDevice {
        device_id: [3u8; 32],
        exchange_public_key: [4u8; 32],
        device_name: "Old Tablet".to_string(),
        created_at: 1699000000,
        revoked: true,
        revoked_at: Some(1700000000),
    };

    let serialized = serde_json::to_string(&device).unwrap();

    assert!(serialized.contains("\"revoked\":true"));
    assert!(serialized.contains("\"revoked_at\":1700000000"));
}

// =============================================================================
// RATCHET MESSAGE SNAPSHOTS
// =============================================================================

#[test]
fn test_ratchet_message_serialization_snapshot() {
use webbook_core::crypto::ratchet::RatchetMessage;

    let msg = RatchetMessage {
        dh_public: [5u8; 32],
        dh_generation: 10,
        message_index: 25,
        previous_chain_length: 20,
        ciphertext: vec![1, 2, 3, 4, 5],
    };

    let serialized = serde_json::to_string(&msg).unwrap();

    // Verify structure
    assert!(serialized.contains("\"dh_public\""));
    assert!(serialized.contains("\"dh_generation\":10"));
    assert!(serialized.contains("\"message_index\":25"));
    assert!(serialized.contains("\"previous_chain_length\":20"));
    assert!(serialized.contains("\"ciphertext\""));

    // Round-trip
    let reparsed: RatchetMessage = serde_json::from_str(&serialized).unwrap();
    assert_eq!(reparsed.dh_generation, 10);
    assert_eq!(reparsed.message_index, 25);
    assert_eq!(reparsed.ciphertext, vec![1, 2, 3, 4, 5]);
}

// =============================================================================
// VISIBILITY RULES SNAPSHOTS
// =============================================================================

#[test]
fn test_field_visibility_everyone_serialization_snapshot() {
use webbook_core::FieldVisibility;

    let visibility = FieldVisibility::Everyone;
    let serialized = serde_json::to_string(&visibility).unwrap();

    assert_eq!(serialized, "\"Everyone\"");
}

#[test]
fn test_field_visibility_nobody_serialization_snapshot() {
use webbook_core::FieldVisibility;

    let visibility = FieldVisibility::Nobody;
    let serialized = serde_json::to_string(&visibility).unwrap();

    assert_eq!(serialized, "\"Nobody\"");
}

#[test]
fn test_field_visibility_contacts_serialization_snapshot() {
use std::collections::HashSet;
use webbook_core::FieldVisibility;

    let mut contacts = HashSet::new();
    contacts.insert("contact-1".to_string());
    contacts.insert("contact-2".to_string());

    let visibility = FieldVisibility::Contacts(contacts);
    let serialized = serde_json::to_string(&visibility).unwrap();

    // Verify structure (order may vary due to HashSet)
    assert!(serialized.contains("\"Contacts\""));
    assert!(serialized.contains("contact-1"));
    assert!(serialized.contains("contact-2"));
}

#[test]
fn test_visibility_rules_serialization_snapshot() {
use webbook_core::VisibilityRules;

    let mut rules = VisibilityRules::new();
    rules.set_everyone("email");
    rules.set_nobody("phone");

    let serialized = serde_json::to_string(&rules).unwrap();

    // Verify structure
    assert!(serialized.contains("\"rules\""));
    assert!(serialized.contains("\"email\""));
    assert!(serialized.contains("\"phone\""));
    assert!(serialized.contains("\"Everyone\""));
    assert!(serialized.contains("\"Nobody\""));
}

// =============================================================================
// SOCIAL NETWORK SNAPSHOTS
// =============================================================================

#[test]
fn test_social_network_serialization_snapshot() {
use webbook_core::SocialNetwork;

    let network = SocialNetwork::new("github", "GitHub", "https://github.com/{username}");
    let serialized = serde_json::to_string(&network).unwrap();

    // Verify structure
    assert!(serialized.contains("\"id\":\"github\""));
    assert!(serialized.contains("\"display_name\":\"GitHub\""));
    assert!(serialized.contains("\"profile_url_template\":\"https://github.com/{username}\""));

    // Round-trip
    let reparsed: SocialNetwork = serde_json::from_str(&serialized).unwrap();
    assert_eq!(reparsed.id(), "github");
    assert_eq!(reparsed.display_name(), "GitHub");
}

// =============================================================================
// ARRAY ENCODING SNAPSHOTS
// =============================================================================

#[test]
fn test_byte_array_32_serialization_format() {
use webbook_core::crypto::ratchet::RatchetMessage;

    // Create a message with known bytes
    let msg = RatchetMessage {
        dh_public: [0u8; 32],
        dh_generation: 0,
        message_index: 0,
        previous_chain_length: 0,
        ciphertext: vec![],
    };

    let serialized = serde_json::to_string(&msg).unwrap();

    // dh_public should be serialized as a JSON array of 32 numbers
    // Format: [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]
    let expected_array = "[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]";
    assert!(
        serialized.contains(expected_array),
        "32-byte arrays should serialize as JSON arrays of numbers"
    );
}

#[test]
fn test_byte_vec_serialization_format() {
use webbook_core::crypto::ratchet::RatchetMessage;

    let msg = RatchetMessage {
        dh_public: [0u8; 32],
        dh_generation: 0,
        message_index: 0,
        previous_chain_length: 0,
        ciphertext: vec![72, 101, 108, 108, 111], // "Hello" in bytes
    };

    let serialized = serde_json::to_string(&msg).unwrap();

    // Vec<u8> should serialize as JSON array
    assert!(
        serialized.contains("[72,101,108,108,111]"),
        "Vec<u8> should serialize as JSON array of numbers"
    );
}
