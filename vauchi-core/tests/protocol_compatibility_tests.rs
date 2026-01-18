//! Protocol Compatibility Tests
//!
//! These tests use "golden" fixtures - known-good serialized JSON that current
//! code must continue to deserialize correctly. This prevents accidental breaking
//! changes to wire formats and storage formats.
//!
//! IMPORTANT: Golden fixtures are contracts. Never modify them unless you're
//! intentionally making a breaking protocol change (which requires migration).

// =============================================================================
// GOLDEN FIXTURES - DO NOT MODIFY
// =============================================================================

/// RatchetMessage V1 golden fixture.
/// Wire format for encrypted messages between contacts.
const RATCHET_MESSAGE_V1: &str = r#"{
    "dh_public": [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32],
    "dh_generation": 5,
    "message_index": 12,
    "previous_chain_length": 10,
    "ciphertext": [72,101,108,108,111,32,87,111,114,108,100]
}"#;

/// ContactCard V1 golden fixture.
/// Core data model for contact information.
const CONTACT_CARD_V1: &str = r#"{
    "id": "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6",
    "display_name": "Alice Smith",
    "fields": [
        {
            "id": "f1e2d3c4b5a6",
            "field_type": "Email",
            "label": "Work",
            "value": "alice@example.com"
        },
        {
            "id": "a9b8c7d6e5f4",
            "field_type": "Phone",
            "label": "Mobile",
            "value": "+1-555-123-4567"
        },
        {
            "id": "x1y2z3w4v5u6",
            "field_type": "Social",
            "label": "Twitter",
            "value": "@alicesmith"
        }
    ]
}"#;

/// ContactField with each FieldType variant.
const FIELD_TYPE_PHONE_V1: &str =
    r#"{"id":"f001","field_type":"Phone","label":"Mobile","value":"+1234567890"}"#;
const FIELD_TYPE_EMAIL_V1: &str =
    r#"{"id":"f002","field_type":"Email","label":"Work","value":"test@example.com"}"#;
const FIELD_TYPE_SOCIAL_V1: &str =
    r#"{"id":"f003","field_type":"Social","label":"GitHub","value":"octocat"}"#;
const FIELD_TYPE_ADDRESS_V1: &str =
    r#"{"id":"f004","field_type":"Address","label":"Home","value":"123 Main St"}"#;
const FIELD_TYPE_WEBSITE_V1: &str =
    r#"{"id":"f005","field_type":"Website","label":"Blog","value":"https://example.com"}"#;
const FIELD_TYPE_CUSTOM_V1: &str =
    r#"{"id":"f006","field_type":"Custom","label":"Notes","value":"Custom data"}"#;

/// CardDelta V1 with Added field change.
/// Wire format for contact card updates.
/// Signature is base64-encoded (64 bytes = 88 base64 chars with padding).
const CARD_DELTA_ADDED_V1: &str = r#"{
    "version": 1,
    "timestamp": 1700000000,
    "changes": [
        {
            "Added": {
                "field": {
                    "id": "newfield01",
                    "field_type": "Email",
                    "label": "Personal",
                    "value": "personal@example.com"
                }
            }
        }
    ],
    "signature": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
}"#;

/// CardDelta V1 with Modified field change.
const CARD_DELTA_MODIFIED_V1: &str = r#"{
    "version": 2,
    "timestamp": 1700000100,
    "changes": [
        {
            "Modified": {
                "field_id": "existingfield",
                "new_value": "updated@example.com"
            }
        }
    ],
    "signature": "EREREREREREREREREREREREREREREREREREREREREREREREREREREREREREREREREREREREREREREREREREREQ=="
}"#;

/// CardDelta V1 with Removed field change.
const CARD_DELTA_REMOVED_V1: &str = r#"{
    "version": 3,
    "timestamp": 1700000200,
    "changes": [
        {
            "Removed": {
                "field_id": "oldfield"
            }
        }
    ],
    "signature": "IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIg=="
}"#;

/// CardDelta V1 with DisplayNameChanged.
const CARD_DELTA_NAME_CHANGED_V1: &str = r#"{
    "version": 4,
    "timestamp": 1700000300,
    "changes": [
        {
            "DisplayNameChanged": {
                "new_name": "Alice Johnson"
            }
        }
    ],
    "signature": "MzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMzMw=="
}"#;

/// DeviceRegistry V1 with two devices.
/// Persistence and broadcast format for device management.
const DEVICE_REGISTRY_V1: &str = r#"{
    "devices": [
        {
            "device_id": [1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],
            "exchange_public_key": [2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2],
            "device_name": "Phone",
            "created_at": 1700000000,
            "revoked": false,
            "revoked_at": null
        },
        {
            "device_id": [3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3],
            "exchange_public_key": [4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4,4],
            "device_name": "Laptop",
            "created_at": 1700000100,
            "revoked": false,
            "revoked_at": null
        }
    ],
    "version": 2,
    "signature": "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
}"#;

/// RegisteredDevice V1 with revoked state.
const REGISTERED_DEVICE_REVOKED_V1: &str = r#"{
    "device_id": [5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5,5],
    "exchange_public_key": [6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6,6],
    "device_name": "Old Tablet",
    "created_at": 1699000000,
    "revoked": true,
    "revoked_at": 1700000500
}"#;

/// SyncItem::CardUpdated V1.
/// Device-to-device sync format.
const SYNC_ITEM_CARD_UPDATED_V1: &str = r#"{
    "CardUpdated": {
        "field_label": "email",
        "new_value": "newemail@example.com",
        "timestamp": 1700001000
    }
}"#;

/// SyncItem::ContactRemoved V1.
const SYNC_ITEM_CONTACT_REMOVED_V1: &str = r#"{
    "ContactRemoved": {
        "contact_id": "contact-abc-123",
        "timestamp": 1700002000
    }
}"#;

/// SyncItem::VisibilityChanged V1.
const SYNC_ITEM_VISIBILITY_CHANGED_V1: &str = r#"{
    "VisibilityChanged": {
        "contact_id": "contact-xyz-789",
        "field_label": "phone",
        "is_visible": false,
        "timestamp": 1700003000
    }
}"#;

/// SerializedRatchetState V1.
/// Persistence format for Double Ratchet state.
const SERIALIZED_RATCHET_STATE_V1: &str = r#"{
    "root_key": [10,20,30,40,50,60,70,80,90,100,110,120,130,140,150,160,170,180,190,200,210,220,230,240,250,1,2,3,4,5,6,7],
    "our_dh_secret": [7,6,5,4,3,2,1,0,7,6,5,4,3,2,1,0,7,6,5,4,3,2,1,0,7,6,5,4,3,2,1,0],
    "their_dh": [8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8,8],
    "send_chain": [[9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9], 3],
    "recv_chain": [[10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10,10], 2],
    "dh_generation": 5,
    "send_message_count": 15,
    "recv_message_count": 12,
    "previous_send_chain_length": 10,
    "skipped_keys": [[[4, 7], [11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11]]]
}"#;

// =============================================================================
// TESTS
// =============================================================================

#[test]
fn test_ratchet_message_compatibility_v1() {
    use vauchi_core::crypto::ratchet::RatchetMessage;

    // Must deserialize correctly
    let msg: RatchetMessage = serde_json::from_str(RATCHET_MESSAGE_V1)
        .expect("Failed to deserialize RatchetMessage V1 golden fixture");

    // Verify fields
    assert_eq!(msg.dh_generation, 5);
    assert_eq!(msg.message_index, 12);
    assert_eq!(msg.previous_chain_length, 10);
    assert_eq!(msg.dh_public[0], 1);
    assert_eq!(msg.dh_public[31], 32);
    assert_eq!(msg.ciphertext, b"Hello World".to_vec());

    // Round-trip test
    let reserialized = serde_json::to_string(&msg).unwrap();
    let reparsed: RatchetMessage = serde_json::from_str(&reserialized).unwrap();
    assert_eq!(msg.dh_generation, reparsed.dh_generation);
    assert_eq!(msg.message_index, reparsed.message_index);
    assert_eq!(msg.ciphertext, reparsed.ciphertext);
}

#[test]
fn test_contact_card_compatibility_v1() {
    use vauchi_core::ContactCard;

    // Must deserialize correctly
    let card: ContactCard = serde_json::from_str(CONTACT_CARD_V1)
        .expect("Failed to deserialize ContactCard V1 golden fixture");

    // Verify fields
    assert_eq!(card.id(), "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6");
    assert_eq!(card.display_name(), "Alice Smith");
    assert_eq!(card.fields().len(), 3);

    // Verify field types and values
    let email_field = card.fields().iter().find(|f| f.label() == "Work").unwrap();
    assert_eq!(email_field.value(), "alice@example.com");

    let phone_field = card
        .fields()
        .iter()
        .find(|f| f.label() == "Mobile")
        .unwrap();
    assert_eq!(phone_field.value(), "+1-555-123-4567");

    // Round-trip test
    let reserialized = serde_json::to_string(&card).unwrap();
    let reparsed: ContactCard = serde_json::from_str(&reserialized).unwrap();
    assert_eq!(card.id(), reparsed.id());
    assert_eq!(card.display_name(), reparsed.display_name());
    assert_eq!(card.fields().len(), reparsed.fields().len());
}

#[test]
fn test_field_type_variants_compatibility_v1() {
    use vauchi_core::ContactField;

    // Test each FieldType variant
    let phone: ContactField = serde_json::from_str(FIELD_TYPE_PHONE_V1).unwrap();
    assert_eq!(phone.label(), "Mobile");

    let email: ContactField = serde_json::from_str(FIELD_TYPE_EMAIL_V1).unwrap();
    assert_eq!(email.label(), "Work");

    let social: ContactField = serde_json::from_str(FIELD_TYPE_SOCIAL_V1).unwrap();
    assert_eq!(social.label(), "GitHub");

    let address: ContactField = serde_json::from_str(FIELD_TYPE_ADDRESS_V1).unwrap();
    assert_eq!(address.label(), "Home");

    let website: ContactField = serde_json::from_str(FIELD_TYPE_WEBSITE_V1).unwrap();
    assert_eq!(website.label(), "Blog");

    let custom: ContactField = serde_json::from_str(FIELD_TYPE_CUSTOM_V1).unwrap();
    assert_eq!(custom.label(), "Notes");
}

#[test]
fn test_card_delta_added_compatibility_v1() {
    use vauchi_core::sync::CardDelta;

    let delta: CardDelta = serde_json::from_str(CARD_DELTA_ADDED_V1)
        .expect("Failed to deserialize CardDelta Added V1");

    assert_eq!(delta.version, 1);
    assert_eq!(delta.timestamp, 1700000000);
    assert_eq!(delta.changes.len(), 1);

    // Verify it's an Added change
    match &delta.changes[0] {
        vauchi_core::sync::FieldChange::Added { field } => {
            assert_eq!(field.label(), "Personal");
            assert_eq!(field.value(), "personal@example.com");
        }
        _ => panic!("Expected Added variant"),
    }
}

#[test]
fn test_card_delta_modified_compatibility_v1() {
    use vauchi_core::sync::CardDelta;

    let delta: CardDelta = serde_json::from_str(CARD_DELTA_MODIFIED_V1)
        .expect("Failed to deserialize CardDelta Modified V1");

    assert_eq!(delta.version, 2);
    assert_eq!(delta.changes.len(), 1);

    match &delta.changes[0] {
        vauchi_core::sync::FieldChange::Modified {
            field_id,
            new_value,
        } => {
            assert_eq!(field_id, "existingfield");
            assert_eq!(new_value, "updated@example.com");
        }
        _ => panic!("Expected Modified variant"),
    }
}

#[test]
fn test_card_delta_removed_compatibility_v1() {
    use vauchi_core::sync::CardDelta;

    let delta: CardDelta = serde_json::from_str(CARD_DELTA_REMOVED_V1)
        .expect("Failed to deserialize CardDelta Removed V1");

    assert_eq!(delta.version, 3);

    match &delta.changes[0] {
        vauchi_core::sync::FieldChange::Removed { field_id } => {
            assert_eq!(field_id, "oldfield");
        }
        _ => panic!("Expected Removed variant"),
    }
}

#[test]
fn test_card_delta_display_name_changed_compatibility_v1() {
    use vauchi_core::sync::CardDelta;

    let delta: CardDelta = serde_json::from_str(CARD_DELTA_NAME_CHANGED_V1)
        .expect("Failed to deserialize CardDelta DisplayNameChanged V1");

    assert_eq!(delta.version, 4);

    match &delta.changes[0] {
        vauchi_core::sync::FieldChange::DisplayNameChanged { new_name } => {
            assert_eq!(new_name, "Alice Johnson");
        }
        _ => panic!("Expected DisplayNameChanged variant"),
    }
}

#[test]
fn test_device_registry_compatibility_v1() {
    use vauchi_core::identity::DeviceRegistry;

    let registry: DeviceRegistry =
        serde_json::from_str(DEVICE_REGISTRY_V1).expect("Failed to deserialize DeviceRegistry V1");

    assert_eq!(registry.version(), 2);
    assert_eq!(registry.all_devices().len(), 2);

    // Verify first device
    let phone = &registry.all_devices()[0];
    assert_eq!(phone.device_name, "Phone");
    assert_eq!(phone.created_at, 1700000000);
    assert!(!phone.revoked);

    // Verify second device
    let laptop = &registry.all_devices()[1];
    assert_eq!(laptop.device_name, "Laptop");
    assert_eq!(laptop.created_at, 1700000100);

    // Round-trip test (signature won't verify but serialization should work)
    let reserialized = serde_json::to_string(&registry).unwrap();
    let reparsed: DeviceRegistry = serde_json::from_str(&reserialized).unwrap();
    assert_eq!(registry.version(), reparsed.version());
    assert_eq!(registry.all_devices().len(), reparsed.all_devices().len());
}

#[test]
fn test_registered_device_revoked_compatibility_v1() {
    use vauchi_core::identity::RegisteredDevice;

    let device: RegisteredDevice = serde_json::from_str(REGISTERED_DEVICE_REVOKED_V1)
        .expect("Failed to deserialize RegisteredDevice revoked V1");

    assert_eq!(device.device_name, "Old Tablet");
    assert!(device.revoked);
    assert_eq!(device.revoked_at, Some(1700000500));
    assert!(!device.is_active());
}

#[test]
fn test_sync_item_card_updated_compatibility_v1() {
    use vauchi_core::sync::SyncItem;

    let item: SyncItem = serde_json::from_str(SYNC_ITEM_CARD_UPDATED_V1)
        .expect("Failed to deserialize SyncItem::CardUpdated V1");

    // Test timestamp accessor before match (which moves values)
    assert_eq!(item.timestamp(), 1700001000);

    match item {
        SyncItem::CardUpdated {
            field_label,
            new_value,
            timestamp,
        } => {
            assert_eq!(field_label, "email");
            assert_eq!(new_value, "newemail@example.com");
            assert_eq!(timestamp, 1700001000);
        }
        _ => panic!("Expected CardUpdated variant"),
    }
}

#[test]
fn test_sync_item_contact_removed_compatibility_v1() {
    use vauchi_core::sync::SyncItem;

    let item: SyncItem = serde_json::from_str(SYNC_ITEM_CONTACT_REMOVED_V1)
        .expect("Failed to deserialize SyncItem::ContactRemoved V1");

    match item {
        SyncItem::ContactRemoved {
            contact_id,
            timestamp,
        } => {
            assert_eq!(contact_id, "contact-abc-123");
            assert_eq!(timestamp, 1700002000);
        }
        _ => panic!("Expected ContactRemoved variant"),
    }
}

#[test]
fn test_sync_item_visibility_changed_compatibility_v1() {
    use vauchi_core::sync::SyncItem;

    let item: SyncItem = serde_json::from_str(SYNC_ITEM_VISIBILITY_CHANGED_V1)
        .expect("Failed to deserialize SyncItem::VisibilityChanged V1");

    match item {
        SyncItem::VisibilityChanged {
            contact_id,
            field_label,
            is_visible,
            timestamp,
        } => {
            assert_eq!(contact_id, "contact-xyz-789");
            assert_eq!(field_label, "phone");
            assert!(!is_visible);
            assert_eq!(timestamp, 1700003000);
        }
        _ => panic!("Expected VisibilityChanged variant"),
    }
}

#[test]
fn test_serialized_ratchet_state_compatibility_v1() {
    use vauchi_core::crypto::ratchet::SerializedRatchetState;

    let state: SerializedRatchetState = serde_json::from_str(SERIALIZED_RATCHET_STATE_V1)
        .expect("Failed to deserialize SerializedRatchetState V1");

    // Verify key fields
    assert_eq!(state.dh_generation, 5);
    assert_eq!(state.send_message_count, 15);
    assert_eq!(state.recv_message_count, 12);
    assert_eq!(state.previous_send_chain_length, 10);

    // Verify root key (spot check)
    assert_eq!(state.root_key[0], 10);
    assert_eq!(state.root_key[31], 7);

    // Verify their_dh is present
    assert!(state.their_dh.is_some());
    assert_eq!(state.their_dh.unwrap()[0], 8);

    // Verify send_chain
    let (send_key, send_gen) = state.send_chain.unwrap();
    assert_eq!(send_key[0], 9);
    assert_eq!(send_gen, 3);

    // Verify recv_chain
    let (recv_key, recv_gen) = state.recv_chain.unwrap();
    assert_eq!(recv_key[0], 10);
    assert_eq!(recv_gen, 2);

    // Verify skipped_keys
    assert_eq!(state.skipped_keys.len(), 1);
    let ((dh_gen, msg_idx), key) = &state.skipped_keys[0];
    assert_eq!(*dh_gen, 4);
    assert_eq!(*msg_idx, 7);
    assert_eq!(key[0], 11);

    // Round-trip test
    let reserialized = serde_json::to_string(&state).unwrap();
    let reparsed: SerializedRatchetState = serde_json::from_str(&reserialized).unwrap();
    assert_eq!(state.dh_generation, reparsed.dh_generation);
    assert_eq!(state.send_message_count, reparsed.send_message_count);
}

// =============================================================================
// ENUM VARIANT NAME STABILITY TESTS
// =============================================================================

/// Verifies that enum variant names haven't changed (which would break JSON).
#[test]
fn test_field_change_enum_variant_names() {
    // These strings represent the JSON enum tag names
    // If these fail to parse, the enum variant names have changed
    let added = r#"{"Added":{"field":{"id":"x","field_type":"Phone","label":"L","value":"V"}}}"#;
    let modified = r#"{"Modified":{"field_id":"x","new_value":"v"}}"#;
    let removed = r#"{"Removed":{"field_id":"x"}}"#;
    let name_changed = r#"{"DisplayNameChanged":{"new_name":"N"}}"#;

    use vauchi_core::sync::FieldChange;

    serde_json::from_str::<FieldChange>(added).expect("Added variant name changed");
    serde_json::from_str::<FieldChange>(modified).expect("Modified variant name changed");
    serde_json::from_str::<FieldChange>(removed).expect("Removed variant name changed");
    serde_json::from_str::<FieldChange>(name_changed)
        .expect("DisplayNameChanged variant name changed");
}

#[test]
fn test_field_type_enum_variant_names() {
    use vauchi_core::FieldType;

    // These must match exactly
    let variants = [
        (r#""Phone""#, FieldType::Phone),
        (r#""Email""#, FieldType::Email),
        (r#""Social""#, FieldType::Social),
        (r#""Address""#, FieldType::Address),
        (r#""Website""#, FieldType::Website),
        (r#""Custom""#, FieldType::Custom),
    ];

    for (json, expected) in variants {
        let parsed: FieldType = serde_json::from_str(json)
            .unwrap_or_else(|_| panic!("FieldType variant {} name changed", json));
        assert_eq!(parsed, expected);
    }
}

#[test]
fn test_sync_item_enum_variant_names() {
    // Verify all SyncItem variant names are stable
    let card_updated = r#"{"CardUpdated":{"field_label":"x","new_value":"y","timestamp":0}}"#;
    let contact_removed = r#"{"ContactRemoved":{"contact_id":"x","timestamp":0}}"#;
    let visibility = r#"{"VisibilityChanged":{"contact_id":"x","field_label":"y","is_visible":true,"timestamp":0}}"#;

    use vauchi_core::sync::SyncItem;

    serde_json::from_str::<SyncItem>(card_updated).expect("CardUpdated variant name changed");
    serde_json::from_str::<SyncItem>(contact_removed).expect("ContactRemoved variant name changed");
    serde_json::from_str::<SyncItem>(visibility).expect("VisibilityChanged variant name changed");
}
