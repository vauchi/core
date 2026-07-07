// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for device sync support for imported contacts (Task 9 / DD-3).
//!
//! Verifies that `ImportedContactSyncData` can be serialized, deserialized,
//! created from a `Contact`, and reconstructed back into a `Contact`.
//! Also checks that the new `SyncItem` variants round-trip correctly and
//! that `from_contact` returns `None` for exchanged (non-imported) contacts.

use vauchi_core::ImportSource;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::sync::{ImportedContactSyncData, SyncItem};

// ── Helpers ─────────────────────────────────────────────────────────────────

fn make_imported(name: &str) -> Contact {
    let card = ContactCard::new(name);
    let contact_id = format!("contact-{name}");
    Contact::from_import(
        contact_id,
        card,
        ImportSource::VcardFile,
        Some(format!("uid-{}", name)),
        1_700_000_000, // Pinned non-zero stamp; test asserts `imported_at > 0`.
    )
}

fn make_exchanged(name: &str) -> Contact {
    let mut public_key = [0u8; 32];
    for (i, &b) in name.as_bytes().iter().enumerate() {
        public_key[i % 32] ^= b;
    }
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key, 0)
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// Serialize then deserialize `ImportedContactSyncData` — all fields survive.
// @internal
#[test]
fn imported_sync_data_roundtrip() {
    let contact = make_imported("Alice");
    let sync_data = ImportedContactSyncData::from_contact(&contact).unwrap();

    let json = serde_json::to_string(&sync_data).unwrap();
    let restored: ImportedContactSyncData = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.id, sync_data.id);
    assert_eq!(restored.display_name, sync_data.display_name);
    assert_eq!(restored.card_json, sync_data.card_json);
    assert_eq!(restored.source, sync_data.source);
    assert_eq!(restored.imported_at, sync_data.imported_at);
    assert_eq!(restored.original_uid, sync_data.original_uid);
}

/// `from_contact` populates all fields from an imported `Contact`.
// @internal
#[test]
fn imported_sync_data_from_contact() {
    let contact = make_imported("Bob");
    let sync_data = ImportedContactSyncData::from_contact(&contact).unwrap();

    assert_eq!(sync_data.id, contact.id());
    assert_eq!(sync_data.display_name, contact.display_name());
    assert_eq!(
        sync_data.original_uid,
        Some("uid-Bob".to_string()),
        "original_uid must match what was passed to from_import"
    );
    assert!(
        sync_data.imported_at > 0,
        "imported_at must be a non-zero unix timestamp"
    );
    // source must be valid JSON for ImportSource::VcardFile
    let _: ImportSource = serde_json::from_str(&sync_data.source)
        .expect("source field must deserialize to ImportSource");
}

/// `to_contact` reconstructs an imported `Contact` with the same id and name.
// @internal
#[test]
fn imported_sync_data_to_contact() {
    let original = make_imported("Carol");
    let sync_data = ImportedContactSyncData::from_contact(&original).unwrap();

    let restored = sync_data.to_contact().unwrap();

    assert_eq!(restored.id(), original.id(), "ID must be preserved");
    assert_eq!(
        restored.display_name(),
        original.display_name(),
        "display_name must be preserved"
    );
    assert!(
        restored.is_imported(),
        "Reconstructed contact must be imported kind"
    );
    assert!(
        !restored.is_exchanged(),
        "Reconstructed contact must NOT be exchanged"
    );

    let imported_data = restored.kind().imported_data().unwrap();
    assert_eq!(imported_data.imported_at, sync_data.imported_at);
    assert_eq!(
        imported_data.original_uid,
        original.kind().imported_data().unwrap().original_uid
    );
}

/// `SyncItem::ImportedContactAdded` serializes and deserializes correctly.
// @internal
#[test]
fn sync_item_imported_added_roundtrip() {
    let contact = make_imported("Dave");
    let contact_data = ImportedContactSyncData::from_contact(&contact).unwrap();
    let item = SyncItem::ImportedContactAdded {
        contact_data: contact_data.clone(),
        timestamp: 1_700_000_000,
    };

    let json = serde_json::to_string(&item).unwrap();
    let restored: SyncItem = serde_json::from_str(&json).unwrap();

    match restored {
        SyncItem::ImportedContactAdded {
            contact_data: restored_data,
            timestamp,
        } => {
            assert_eq!(restored_data.id, contact_data.id);
            assert_eq!(restored_data.display_name, contact_data.display_name);
            assert_eq!(timestamp, 1_700_000_000);
        }
        other => panic!("Expected ImportedContactAdded, got {:?}", other),
    }
}

/// `SyncItem::ImportedContactRemoved` serializes and deserializes correctly.
// @internal
#[test]
fn sync_item_imported_removed_roundtrip() {
    let item = SyncItem::ImportedContactRemoved {
        contact_id: "some-uuid-1234".to_string(),
        timestamp: 1_700_000_001,
    };

    let json = serde_json::to_string(&item).unwrap();
    let restored: SyncItem = serde_json::from_str(&json).unwrap();

    match restored {
        SyncItem::ImportedContactRemoved {
            contact_id,
            timestamp,
        } => {
            assert_eq!(contact_id, "some-uuid-1234");
            assert_eq!(timestamp, 1_700_000_001);
        }
        other => panic!("Expected ImportedContactRemoved, got {:?}", other),
    }
}

/// `from_contact` returns `None` for an exchanged contact — no imported data.
// @internal
#[test]
fn imported_sync_data_returns_none_for_exchanged() {
    let contact = make_exchanged("Eve");
    let result = ImportedContactSyncData::from_contact(&contact);
    assert!(
        result.is_none(),
        "from_contact must return None for an exchanged contact"
    );
}

/// `timestamp()` method works for all three new SyncItem variants.
// @internal
#[test]
fn sync_item_timestamp_for_imported_variants() {
    let contact = make_imported("Frank");
    let contact_data = ImportedContactSyncData::from_contact(&contact).unwrap();

    let added = SyncItem::ImportedContactAdded {
        contact_data: contact_data.clone(),
        timestamp: 100,
    };
    let updated = SyncItem::ImportedContactUpdated {
        contact_data,
        timestamp: 200,
    };
    let removed = SyncItem::ImportedContactRemoved {
        contact_id: "x".to_string(),
        timestamp: 300,
    };

    assert_eq!(added.timestamp(), 100);
    assert_eq!(updated.timestamp(), 200);
    assert_eq!(removed.timestamp(), 300);
}
