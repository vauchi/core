// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for sync::delta
//! Extracted from delta.rs

use vauchi_core::contact_card::FieldType;
use vauchi_core::sync::*;
use vauchi_core::*;

// @scenario: sync_updates :: Only changed fields transmitted
// @internal
#[test]
fn test_delta_compute_no_changes() {
    let card = ContactCard::new("Alice");
    let delta = CardDelta::compute(&card, &card, 0);

    assert!(delta.is_empty());
}

// @scenario: sync_updates :: Only changed fields transmitted
// @internal
#[test]
fn test_delta_compute_display_name_change() {
    let old = ContactCard::new("Alice");
    let new = ContactCard::new("Alice Smith");

    let delta = CardDelta::compute(&old, &new, 0);

    assert_eq!(delta.changes.len(), 1);
    assert!(matches!(
        &delta.changes[0],
        FieldChange::DisplayNameChanged { new_name } if new_name == "Alice Smith"
    ));
}

// @scenario: sync_updates :: Only changed fields transmitted
// @internal
#[test]
fn test_delta_compute_field_added() {
    let old = ContactCard::new("Alice");

    let mut new = ContactCard::new("Alice");
    let _ = new.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
        0,
    ));

    let delta = CardDelta::compute(&old, &new, 0);

    assert_eq!(delta.changes.len(), 1);
    assert!(matches!(&delta.changes[0], FieldChange::Added { .. }));
}

// @internal
#[test]
fn test_delta_compute_field_modified() {
    let mut old = ContactCard::new("Alice");
    let _ = old.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "old@example.com",
        0,
    ));

    let mut new = ContactCard::new("Alice");
    let _ = new.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "new@example.com",
        0,
    ));

    let delta = CardDelta::compute(&old, &new, 0);

    // The field IDs are generated, so both have different IDs
    // This will show as added + removed rather than modified
    // For true modification tracking, we'd need stable field IDs
    assert!(!delta.is_empty());
}

// @scenario: sync_updates :: Only changed fields transmitted
// @internal
#[test]
fn test_delta_compute_field_removed() {
    let mut old = ContactCard::new("Alice");
    let field = ContactField::new(FieldType::Email, "email", "alice@example.com", 0);
    let field_id = field.id().to_string();
    let _ = old.add_field(field);

    let new = ContactCard::new("Alice");

    let delta = CardDelta::compute(&old, &new, 0);

    assert_eq!(delta.changes.len(), 1);
    assert!(matches!(
        &delta.changes[0],
        FieldChange::Removed { field_id: id } if *id == field_id
    ));
}

// @scenario: sync_updates :: Receive contact card update
// @internal
#[test]
fn test_delta_apply_display_name() {
    let mut card = ContactCard::new("Alice");

    let delta = CardDelta {
        version: 1,
        timestamp: 12345,
        changes: vec![FieldChange::DisplayNameChanged {
            new_name: "Alice Smith".to_string(),
        }],
        nonce: [0u8; 32],
        signature: [0u8; 64],
        validation_summary: None,
    };

    delta.apply(&mut card, 0).unwrap();

    assert_eq!(card.display_name(), "Alice Smith");
}

// @scenario: sync_updates :: Receive contact card update
// @internal
#[test]
fn test_delta_apply_add_field() {
    let mut card = ContactCard::new("Alice");
    let new_field = ContactField::new(FieldType::Email, "email", "alice@example.com", 0);

    let delta = CardDelta {
        version: 1,
        timestamp: 12345,
        changes: vec![FieldChange::Added { field: new_field }],
        nonce: [0u8; 32],
        signature: [0u8; 64],
        validation_summary: None,
    };

    delta.apply(&mut card, 0).unwrap();

    assert_eq!(card.fields().len(), 1);
    assert_eq!(card.fields()[0].value(), "alice@example.com");
}

// @scenario: sync_updates :: Receive contact card update
// @internal
#[test]
fn test_delta_apply_remove_field() {
    let mut card = ContactCard::new("Alice");
    let field = ContactField::new(FieldType::Email, "email", "alice@example.com", 0);
    let field_id = field.id().to_string();
    let _ = card.add_field(field);

    let delta = CardDelta {
        version: 1,
        timestamp: 12345,
        changes: vec![FieldChange::Removed { field_id }],
        nonce: [0u8; 32],
        signature: [0u8; 64],
        validation_summary: None,
    };

    delta.apply(&mut card, 0).unwrap();

    assert!(card.fields().is_empty());
}

// @internal
#[test]
fn test_delta_roundtrip() {
    let mut old = ContactCard::new("Alice");
    let _ = old.add_field(ContactField::new(
        FieldType::Phone,
        "phone",
        "+1234567890",
        0,
    ));

    let mut new = ContactCard::new("Alice Smith");
    let _ = new.add_field(ContactField::new(
        FieldType::Phone,
        "phone",
        "+1234567890",
        0,
    ));
    let _ = new.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
        0,
    ));

    let delta = CardDelta::compute(&old, &new, 0);

    // Apply to a copy of old
    let mut result = old.clone();
    delta.apply(&mut result, 0).unwrap();

    assert_eq!(result.display_name(), "Alice Smith");
    assert_eq!(result.fields().len(), 2);
}

// @scenario: sync_updates :: Verify update signatures
// @internal
#[test]
fn test_delta_sign_and_verify() {
    let identity = Identity::create("Test User");

    let old = ContactCard::new("Alice");
    let new = ContactCard::new("Alice Smith");

    let mut delta = CardDelta::compute(&old, &new, 0);
    let recipient_pk = &[0u8; 32];
    delta.sign(&identity, recipient_pk);

    // Verify with correct public key
    assert!(delta.verify(identity.signing_public_key(), recipient_pk));

    // Verify with wrong sender key should fail
    let other_identity = Identity::create("Other User");
    assert!(!delta.verify(other_identity.signing_public_key(), recipient_pk));

    // Verify with wrong recipient key should fail (prevents delta forwarding)
    let wrong_recipient = &[0xFF; 32];
    assert!(
        !delta.verify(identity.signing_public_key(), wrong_recipient),
        "Delta signed for one recipient must not verify for a different recipient"
    );
}

// @scenario: sync_updates :: Verify update signatures
// @internal
#[test]
fn test_delta_signature_binds_sender_and_recipient() {
    let alice = Identity::create("Alice");
    let bob = Identity::create("Bob");
    let carol = Identity::create("Carol");

    let old = ContactCard::new("Alice");
    let new = ContactCard::new("Alice Updated");

    // Alice signs delta for Bob
    let mut delta = CardDelta::compute(&old, &new, 0);
    delta.sign(&alice, bob.signing_public_key());

    // Verifies correctly for Alice → Bob
    assert!(delta.verify(alice.signing_public_key(), bob.signing_public_key()));

    // Fails for Alice → Carol (wrong recipient)
    assert!(
        !delta.verify(alice.signing_public_key(), carol.signing_public_key()),
        "Delta forwarded to wrong recipient must not verify"
    );

    // Fails for Bob → Bob (wrong sender)
    assert!(!delta.verify(bob.signing_public_key(), bob.signing_public_key()));
}

// @internal
#[test]
fn test_delta_serialization_roundtrip() {
    let mut old = ContactCard::new("Alice");
    let _ = old.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "old@example.com",
        0,
    ));

    let mut new = ContactCard::new("Alice");
    let _ = new.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "new@example.com",
        0,
    ));

    let delta = CardDelta::compute(&old, &new, 0);

    let json = serde_json::to_string(&delta).unwrap();
    let restored: CardDelta = serde_json::from_str(&json).unwrap();

    assert_eq!(delta.version, restored.version);
    assert_eq!(delta.timestamp, restored.timestamp);
    assert_eq!(delta.changes.len(), restored.changes.len());
}

// @internal
#[test]
fn test_delta_multiple_changes() {
    let mut old = ContactCard::new("Alice");
    let field1 = ContactField::new(FieldType::Email, "email", "alice@example.com", 0);
    let field1_id = field1.id().to_string();
    let _ = old.add_field(field1);

    let mut new = ContactCard::new("Alice Smith");
    // email field is removed, phone is added
    let _ = new.add_field(ContactField::new(
        FieldType::Phone,
        "phone",
        "+1234567890",
        0,
    ));

    let delta = CardDelta::compute(&old, &new, 0);

    // Should have: DisplayNameChanged, Removed (email), Added (phone)
    assert_eq!(delta.changes.len(), 3);

    let has_name_change = delta.changes.iter().any(
        |c| matches!(c, FieldChange::DisplayNameChanged { new_name } if new_name == "Alice Smith"),
    );
    assert!(has_name_change);

    let has_removed = delta
        .changes
        .iter()
        .any(|c| matches!(c, FieldChange::Removed { field_id } if *field_id == field1_id));
    assert!(has_removed);

    let has_added = delta
        .changes
        .iter()
        .any(|c| matches!(c, FieldChange::Added { .. }));
    assert!(has_added);
}

// @scenario: sync_updates :: Update only visible fields
// @internal
#[test]
fn test_delta_filter_for_contact_all_visible() {
    use vauchi_core::contact::VisibilityRules;

    let old = ContactCard::new("Alice");
    let mut new = ContactCard::new("Alice");
    let _ = new.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
        0,
    ));
    let _ = new.add_field(ContactField::new(
        FieldType::Phone,
        "phone",
        "+1234567890",
        0,
    ));

    let delta = CardDelta::compute(&old, &new, 0);
    let rules = VisibilityRules::new(); // Default: everyone can see all

    let filtered = delta.filter_for_contact("bob", &rules);

    // Bob should see both fields (default visibility is Everyone)
    assert_eq!(filtered.changes.len(), 2);
}

// @scenario: sync_updates :: Update only visible fields
// @internal
#[test]
fn test_delta_filter_for_contact_some_hidden() {
    use vauchi_core::contact::VisibilityRules;

    let old = ContactCard::new("Alice");
    let mut new = ContactCard::new("Alice");
    let email_field = ContactField::new(FieldType::Email, "email", "alice@example.com", 0);
    let email_id = email_field.id().to_string();
    let _ = new.add_field(email_field);
    let _ = new.add_field(ContactField::new(
        FieldType::Phone,
        "phone",
        "+1234567890",
        0,
    ));

    let delta = CardDelta::compute(&old, &new, 0);

    // Hide email from Bob
    let mut rules = VisibilityRules::new();
    rules.set_nobody(&email_id);

    let filtered = delta.filter_for_contact("bob", &rules);

    // Bob should only see the phone field
    assert_eq!(filtered.changes.len(), 1);
    assert!(
        matches!(&filtered.changes[0], FieldChange::Added { field } if field.label() == "phone")
    );
}

// @scenario: sync_updates :: Update only visible fields
// @internal
#[test]
fn test_delta_filter_for_contact_restricted_access() {
    use std::collections::HashSet;
    use vauchi_core::contact::VisibilityRules;

    let old = ContactCard::new("Alice");
    let mut new = ContactCard::new("Alice");
    let email_field = ContactField::new(FieldType::Email, "email", "alice@example.com", 0);
    let email_id = email_field.id().to_string();
    let _ = new.add_field(email_field);

    let delta = CardDelta::compute(&old, &new, 0);

    // Email only visible to specific contacts
    let mut rules = VisibilityRules::new();
    let mut allowed = HashSet::new();
    allowed.insert("charlie".to_string());
    rules.set_contacts(&email_id, allowed);

    // Bob is not in the allowed list
    let bob_filtered = delta.filter_for_contact("bob", &rules);
    assert!(bob_filtered.is_empty());

    // Charlie is in the allowed list
    let charlie_filtered = delta.filter_for_contact("charlie", &rules);
    assert_eq!(charlie_filtered.changes.len(), 1);
}

// @scenario: sync_updates :: Update only visible fields
// @internal
#[test]
fn test_delta_filter_display_name_always_visible() {
    use vauchi_core::contact::VisibilityRules;

    let old = ContactCard::new("Alice");
    let new = ContactCard::new("Alice Smith");

    let delta = CardDelta::compute(&old, &new, 0);
    let rules = VisibilityRules::new();

    let filtered = delta.filter_for_contact("bob", &rules);

    // Display name changes are always visible
    assert_eq!(filtered.changes.len(), 1);
    assert!(matches!(
        &filtered.changes[0],
        FieldChange::DisplayNameChanged { .. }
    ));
}

// ============================================================
// filter_with() tests
// ============================================================

// @internal
#[test]
fn test_filter_with_allows_matching_fields() {
    // Create old card with two fields
    let work_field = ContactField::new(FieldType::Email, "work", "old@co.com", 0);
    let work_id = work_field.id().to_string();
    let mobile_field = ContactField::new(FieldType::Phone, "mobile", "+1234567890", 0);
    let mobile_id = mobile_field.id().to_string();

    let mut old = ContactCard::new("Alice");
    old.add_field(work_field).expect("add work field");
    old.add_field(mobile_field).expect("add mobile field");

    // Create new card by cloning and modifying both fields
    let mut new = old.clone();
    for field in new.fields_mut() {
        field.set_value(
            if field.id() == work_id {
                "new@co.com"
            } else {
                "+9876543210"
            },
            0,
        );
    }

    let delta = CardDelta::compute(&old, &new, 0);
    // Both fields should be detected as modified
    let change_count = delta.changes.len();
    assert!(
        change_count >= 1,
        "Should detect at least 1 change, got {}",
        change_count
    );

    // Filter: only allow the work field
    let work_id_clone = work_id.clone();
    let filtered = delta.filter_with(|field_id| field_id == work_id_clone);

    // Count how many changes match the work field
    let work_changes: Vec<_> = filtered
        .changes
        .iter()
        .filter(|c| matches!(c, FieldChange::Modified { field_id, .. } if field_id == &work_id))
        .collect();

    // The mobile field changes should be filtered out
    let mobile_changes: Vec<_> = filtered
        .changes
        .iter()
        .filter(|c| matches!(c, FieldChange::Modified { field_id, .. } if field_id == &mobile_id))
        .collect();

    assert!(
        !work_changes.is_empty(),
        "Work field changes should be included"
    );
    assert!(
        mobile_changes.is_empty(),
        "Mobile field changes should be filtered out"
    );
}

// @internal
#[test]
fn test_filter_with_always_includes_display_name() {
    let old = ContactCard::new("Alice");
    let new = ContactCard::new("Alice Smith");

    let delta = CardDelta::compute(&old, &new, 0);
    assert_eq!(delta.changes.len(), 1);

    // Filter that rejects everything — display name should still pass
    let filtered = delta.filter_with(|_| false);
    assert_eq!(filtered.changes.len(), 1);
    assert!(matches!(
        &filtered.changes[0],
        FieldChange::DisplayNameChanged { .. }
    ));
}

// @internal
#[test]
fn test_filter_with_handles_added_and_removed() {
    // Start with a card containing work+mobile
    let work_field = ContactField::new(FieldType::Email, "work", "a@co.com", 0);
    let work_id = work_field.id().to_string();
    let mobile_field = ContactField::new(FieldType::Phone, "mobile", "+1234567890", 0);
    let mobile_id = mobile_field.id().to_string();

    let mut old = ContactCard::new("Alice");
    old.add_field(work_field).expect("add work field");
    old.add_field(mobile_field).expect("add mobile field");

    assert_eq!(old.fields().len(), 2, "old should have 2 fields");

    // New card only has work (mobile was removed)
    let mut new = old.clone();
    new.remove_field(&mobile_id)
        .expect("remove_field should succeed");
    assert_eq!(
        new.fields().len(),
        1,
        "new should have 1 field after removal"
    );

    let delta = CardDelta::compute(&old, &new, 0);
    eprintln!("Delta changes: {:?}", delta.changes);
    assert_eq!(
        delta.changes.len(),
        1,
        "Should have exactly 1 Removed change"
    );
    assert!(
        delta
            .changes
            .iter()
            .any(|c| matches!(c, FieldChange::Removed { field_id } if field_id == &mobile_id)),
        "Should have Removed change for mobile field"
    );

    // Filter: allow mobile_id (removed field passes through)
    let mobile_id_clone = mobile_id.clone();
    let filtered = delta.filter_with(|field_id| field_id == mobile_id_clone);
    assert_eq!(filtered.changes.len(), 1);

    // Filter: allow only work_id (removed mobile field is filtered out)
    let filtered2 = delta.filter_with(|field_id| field_id == work_id);
    assert_eq!(filtered2.changes.len(), 0);
}

// === Zero Signature Rejection Tests (Item 102) ===

// @scenario: sync_updates :: Verify update signatures
// @internal
#[test]
fn test_unsigned_delta_rejected_by_verify() {
    let identity = Identity::create("Alice");

    let old = ContactCard::new("Alice");
    let new = ContactCard::new("Alice Updated");

    // Create delta but do NOT sign it — signature is [0u8; 64]
    let delta = CardDelta::compute(&old, &new, 0);
    assert_eq!(
        delta.signature, [0u8; 64],
        "Unsigned delta should have zero signature"
    );

    // verify() must reject a zero-signature delta
    assert!(
        !delta.verify(identity.signing_public_key(), &[0u8; 32]),
        "Unsigned delta with zero signature must be rejected"
    );
}

// === ValidationSummary in CardDelta Tests (Task 7 — G3) ===

// @scenario: field_validation :: Validation counts in card updates
// @internal
#[test]
fn test_card_delta_with_validation_summary_roundtrip() {
    use std::collections::HashMap;
    use vauchi_core::sync::delta::ValidationSummary;

    let old = ContactCard::new("Bob");
    let mut new = ContactCard::new("Bob");
    let _ = new.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "bob@example.com",
        0,
    ));

    let mut delta = CardDelta::compute(&old, &new, 0);

    // Attach validation summary
    let mut summary = HashMap::new();
    summary.insert(
        "email-field-1".to_string(),
        ValidationSummary {
            count: 3,
            trust_level: "verified".to_string(),
        },
    );
    delta.validation_summary = Some(summary);

    // Serialize to JSON
    let json = serde_json::to_string(&delta).expect("serialize");

    // Deserialize back
    let restored: CardDelta = serde_json::from_str(&json).expect("deserialize");

    // Assert the summary is preserved
    let restored_summary = restored
        .validation_summary
        .expect("validation_summary should be Some after roundtrip");
    assert_eq!(restored_summary.len(), 1);
    let entry = restored_summary
        .get("email-field-1")
        .expect("should have email-field-1 entry");
    assert_eq!(entry.count, 3);
    assert_eq!(entry.trust_level, "verified");
}

// @scenario: field_validation :: Backward compatible card updates
// @internal
#[test]
fn test_card_delta_without_summary_backward_compat() {
    // Simulate a JSON payload from an older client that has no validation_summary field
    let json = r#"{
        "version": 1,
        "timestamp": 12345,
        "changes": [],
        "nonce": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "signature": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
    }"#;

    let delta: CardDelta = serde_json::from_str(json).expect("deserialize legacy delta");

    assert!(
        delta.validation_summary.is_none(),
        "Legacy delta without validation_summary field should deserialize with None"
    );
}

// @scenario: field_validation :: Validation summary not serialized when empty
// @internal
#[test]
fn test_card_delta_none_summary_not_serialized() {
    let old = ContactCard::new("Alice");
    let new = ContactCard::new("Alice Updated");

    let delta = CardDelta::compute(&old, &new, 0);

    // By default, validation_summary should be None
    assert!(
        delta.validation_summary.is_none(),
        "Freshly computed delta should have no validation_summary"
    );

    // When serialized, the JSON should NOT contain the validation_summary key
    let json = serde_json::to_string(&delta).expect("serialize");
    assert!(
        !json.contains("validation_summary"),
        "JSON should not contain validation_summary when it is None (skip_serializing_if)"
    );
}

// @scenario: field_validation :: Validation summary in filtered deltas
// @internal
#[test]
fn test_card_delta_filter_preserves_validation_summary() {
    use std::collections::HashMap;
    use vauchi_core::contact::VisibilityRules;
    use vauchi_core::sync::delta::ValidationSummary;

    let old = ContactCard::new("Alice");
    let mut new = ContactCard::new("Alice");
    let email_field = ContactField::new(FieldType::Email, "email", "alice@example.com", 0);
    let email_id = email_field.id().to_string();
    let _ = new.add_field(email_field);

    let mut delta = CardDelta::compute(&old, &new, 0);

    // Attach validation summary
    let mut summary = HashMap::new();
    summary.insert(
        email_id.clone(),
        ValidationSummary {
            count: 5,
            trust_level: "trusted".to_string(),
        },
    );
    delta.validation_summary = Some(summary);

    let rules = VisibilityRules::new();
    let filtered = delta.filter_for_contact("bob", &rules);

    // The filtered delta should preserve the validation_summary
    assert!(
        filtered.validation_summary.is_some(),
        "Filtered delta should preserve validation_summary"
    );
    let filtered_summary = filtered.validation_summary.unwrap();
    assert_eq!(filtered_summary.len(), 1);
    assert_eq!(filtered_summary.get(&email_id).unwrap().count, 5);
}

// @scenario: field_validation :: Validation summary with multiple fields
// @internal
#[test]
fn test_card_delta_validation_summary_multiple_fields() {
    use std::collections::HashMap;
    use vauchi_core::sync::delta::ValidationSummary;

    let mut summary = HashMap::new();
    summary.insert(
        "field-email".to_string(),
        ValidationSummary {
            count: 7,
            trust_level: "verified".to_string(),
        },
    );
    summary.insert(
        "field-phone".to_string(),
        ValidationSummary {
            count: 2,
            trust_level: "unverified".to_string(),
        },
    );
    summary.insert(
        "field-address".to_string(),
        ValidationSummary {
            count: 0,
            trust_level: "none".to_string(),
        },
    );

    let old = ContactCard::new("Carol");
    let new = ContactCard::new("Carol Updated");

    let mut delta = CardDelta::compute(&old, &new, 0);
    delta.validation_summary = Some(summary);

    let json = serde_json::to_string(&delta).expect("serialize");
    let restored: CardDelta = serde_json::from_str(&json).expect("deserialize");

    let restored_summary = restored.validation_summary.expect("should have summary");
    assert_eq!(restored_summary.len(), 3);
    assert_eq!(restored_summary["field-email"].count, 7);
    assert_eq!(restored_summary["field-phone"].count, 2);
    assert_eq!(restored_summary["field-address"].count, 0);
    assert_eq!(restored_summary["field-address"].trust_level, "none");
}
