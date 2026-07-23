// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contract tests: ContactField.note must NEVER appear in outbound card deltas.
//! These tests guard the privacy boundary between local annotations and
//! data shared with contacts.

use vauchi_core::contact_card::FieldType;
use vauchi_core::sync::delta::{CardDelta, FieldChange};
use vauchi_core::*;

const SECRET_NOTE: &str = "SECRET NOTE DO NOT LEAK 7f3a9b2c";

/// SECURITY: Proves that `CardDelta::compute` strips private annotations
/// from ContactField before placing them in `FieldChange::Added`.
// @internal
#[test]
fn test_card_delta_never_contains_field_notes() {
    let old_card = ContactCard::new("Alice");
    let mut new_card = old_card.clone();

    let field = ContactField::new(FieldType::Phone, "Work", "+41 79 123 45 67", 0)
        .with_note(SECRET_NOTE.to_string());
    new_card.add_field(field).unwrap();

    let delta = CardDelta::compute(&old_card, &new_card, 0);

    // Structural check: Added fields must have note stripped
    for change in &delta.changes {
        if let FieldChange::Added { field } = change {
            assert!(
                field.note().is_none(),
                "SECURITY: Field note leaked in FieldChange::Added — \
                 CardDelta::compute must call strip_private()"
            );
        }
    }

    // Serialization check: note string must not appear anywhere in serialized delta
    let serialized = serde_json::to_string(&delta).unwrap();
    assert!(
        !serialized.contains("SECRET NOTE"),
        "SECURITY: Note string found in serialized CardDelta"
    );
    assert!(
        !serialized.contains("7f3a9b2c"),
        "SECURITY: Note marker found in serialized CardDelta"
    );
}

/// SECURITY: Filtered deltas (via `filter_with`) must also be free of notes.
/// Since stripping happens at compute time, the clones in filter paths inherit
/// the clean fields — this test locks that invariant.
// @internal
#[test]
fn test_filtered_delta_preserves_privacy() {
    let old_card = ContactCard::new("Alice");
    let mut new_card = old_card.clone();

    let field = ContactField::new(FieldType::Email, "Personal", "alice@example.com", 0)
        .with_note(SECRET_NOTE.to_string());
    new_card.add_field(field).unwrap();

    let delta = CardDelta::compute(&old_card, &new_card, 0);

    // filter_with that allows everything — notes must still be absent
    let filtered = delta.filter_with(|_field_id| true);

    for change in &filtered.changes {
        if let FieldChange::Added { field } = change {
            assert!(
                field.note().is_none(),
                "SECURITY: Field note leaked through filter_with() path"
            );
        }
    }

    let serialized = serde_json::to_string(&filtered).unwrap();
    assert!(
        !serialized.contains("SECRET NOTE"),
        "SECURITY: Note string found in filtered delta serialization"
    );
    assert!(
        !serialized.contains("7f3a9b2c"),
        "SECURITY: Note marker found in filtered delta serialization"
    );
}

/// SECURITY: A changed field uses the complete-field upsert path, which must
/// strip owner-only annotations just like a newly added field.
// @internal
#[test]
fn test_modified_field_upsert_never_contains_field_notes() {
    let mut old_card = ContactCard::new("Alice");
    let field = ContactField::new(FieldType::Phone, "Work", "+41 79 123 45 67", 10)
        .with_note(SECRET_NOTE.to_string());
    let field_id = field.id().to_string();
    old_card.add_field(field).unwrap();

    let mut new_card = old_card.clone();
    new_card
        .update_field_value(&field_id, "+41 79 765 43 21", 20)
        .unwrap();

    let delta = CardDelta::compute(&old_card, &new_card, 20);

    assert_eq!(delta.changes.len(), 1);
    match &delta.changes[0] {
        FieldChange::Added { field } => {
            assert_eq!(field.id(), field_id);
            assert_eq!(field.value(), "+41 79 765 43 21");
            assert_eq!(field.updated_at(), 20);
            assert_eq!(field.note(), None);
        }
        other => panic!("expected timestamp-preserving field upsert, got {other:?}"),
    }
    let serialized = serde_json::to_string(&delta).unwrap();
    assert!(!serialized.contains(SECRET_NOTE));
}
