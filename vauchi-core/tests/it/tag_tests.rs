// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact tags (owner-private annotation vocabulary) — the `Tag`
//! domain type and its encrypted storage CRUD (`tags` table, migration v49).
//! See `ADR-051`.

use std::collections::BTreeSet;

use proptest::prelude::*;
use vauchi_core::contact::Tag;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;
use vauchi_core::sync::device_sync::TagSyncData;

fn open_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn new_tag_has_name_empty_membership_and_stamped_time() {
    let tag = Tag::new("climbing-gym", 1_700_000_000);

    assert_eq!(tag.name, "climbing-gym");
    assert!(
        tag.contact_ids.is_empty(),
        "new tag starts with no contacts"
    );
    assert_eq!(tag.created_at, 1_700_000_000);
    assert!(!tag.id.is_empty(), "tag must have a generated id");
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn two_tags_get_distinct_ids() {
    let a = Tag::new("work", 0);
    let b = Tag::new("work", 0);

    assert_ne!(a.id, b.id, "each Tag::new must generate a fresh UUID");
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn add_contact_is_idempotent_and_reports_newness() {
    let mut tag = Tag::new("berlin-trip", 0);

    assert!(tag.add_contact("c1"), "first add reports newly added");
    assert!(
        !tag.add_contact("c1"),
        "re-adding the same contact reports false"
    );
    assert_eq!(tag.contact_ids.len(), 1, "no duplicate membership");
    assert!(tag.contains("c1"));
    assert!(!tag.contains("c2"));
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn remove_contact_reports_presence() {
    let mut tag = Tag::new("berlin-trip", 0);
    tag.add_contact("c1");

    assert!(
        tag.remove_contact("c1"),
        "removing present contact reports true"
    );
    assert!(
        !tag.remove_contact("c1"),
        "removing absent contact reports false"
    );
    assert!(!tag.contains("c1"));
    assert!(tag.contact_ids.is_empty());
}

// ── Storage CRUD (encrypted name, migration v49) ──────────────────────────────

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn create_tag_round_trips_through_get() {
    let storage = open_storage();

    let created = storage.create_tag("climbing-gym").unwrap();
    let loaded = storage.get_tag(&created.id).unwrap().unwrap();

    assert_eq!(loaded.id, created.id);
    assert_eq!(loaded.name, "climbing-gym", "name must decrypt back");
    assert!(loaded.contact_ids.is_empty());
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn get_missing_tag_returns_none() {
    let storage = open_storage();
    assert!(storage.get_tag("does-not-exist").unwrap().is_none());
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn list_tags_returns_all_created() {
    let storage = open_storage();
    storage.create_tag("work").unwrap();
    storage.create_tag("family").unwrap();

    let names: Vec<String> = storage
        .list_tags()
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();

    assert_eq!(names.len(), 2);
    assert!(names.contains(&"work".to_string()));
    assert!(names.contains(&"family".to_string()));
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn add_and_remove_membership_persists() {
    let storage = open_storage();
    let tag = storage.create_tag("berlin-trip").unwrap();

    storage.add_to_tag(&tag.id, "c1").unwrap();
    storage.add_to_tag(&tag.id, "c1").unwrap(); // idempotent
    storage.add_to_tag(&tag.id, "c2").unwrap();

    let loaded = storage.get_tag(&tag.id).unwrap().unwrap();
    assert_eq!(loaded.contact_ids.len(), 2, "no duplicate membership");
    assert!(loaded.contains("c1") && loaded.contains("c2"));

    storage.remove_from_tag(&tag.id, "c1").unwrap();
    let after = storage.get_tag(&tag.id).unwrap().unwrap();
    assert!(!after.contains("c1"));
    assert!(after.contains("c2"));
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn delete_tag_reports_existence_and_removes() {
    let storage = open_storage();
    let tag = storage.create_tag("temp").unwrap();

    assert!(
        storage.delete_tag(&tag.id).unwrap(),
        "delete reports existed"
    );
    assert!(storage.get_tag(&tag.id).unwrap().is_none());
    assert!(
        !storage.delete_tag(&tag.id).unwrap(),
        "second delete reports absent"
    );
}

// @scenario: contact-annotations.feature - Tags are never shared (at-rest)
// @internal
#[test]
fn tag_name_is_encrypted_at_rest() {
    let storage = open_storage();
    let tag = storage.create_tag("ex-colleague").unwrap();

    // Read the raw BLOB straight from the table — it must NOT contain the
    // plaintext name (ADR-051: tag names encrypted at rest).
    let raw: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT name_encrypted FROM tags WHERE id = ?1",
            rusqlite::params![tag.id],
            |row| row.get(0),
        )
        .unwrap();

    let needle = b"ex-colleague";
    assert!(
        !raw.windows(needle.len()).any(|w| w == needle),
        "plaintext tag name must not appear in the stored BLOB"
    );
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn tag_name_survives_storage_rekey() {
    let mut storage = open_storage();
    let tag = storage.create_tag("berlin-trip").unwrap();
    storage.add_to_tag(&tag.id, "c1").unwrap();

    // Rotate the storage key — rekey must re-encrypt the tag name.
    storage.rekey(SymmetricKey::generate()).unwrap();

    let loaded = storage.get_tag(&tag.id).unwrap().unwrap();
    assert_eq!(loaded.name, "berlin-trip", "name must decrypt after rekey");
    assert!(loaded.contains("c1"), "membership preserved across rekey");
}

// ── Adversarial names (CC-14): round-trip through encrypted storage ────────────

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn create_tag_round_trips_adversarial_names() {
    let storage = open_storage();
    let long = "x".repeat(2000);
    let payloads: [&str; 7] = [
        "a",                            // minimal
        &long,                          // very long
        "café — naïve 日本語 🏷",        // unicode + emoji
        "null\u{0}byte",                // embedded NUL
        "Robert'); DROP TABLE tags;--", // SQL-injection shape
        "  internal   spaces   here",   // internal whitespace preserved
        "\u{202e}rtl-override",         // unicode control char
    ];
    for name in payloads {
        let tag = storage.create_tag(name).unwrap();
        let loaded = storage.get_tag(&tag.id).unwrap().unwrap();
        assert_eq!(&loaded.name, name, "name must round-trip exactly: {name:?}");
    }
    assert_eq!(storage.list_tags().unwrap().len(), payloads.len());
}

// ── Property: TagSyncData round-trips (CC-04) ─────────────────────────────────

fn name_strategy() -> impl Strategy<Value = String> {
    "[\\PC]{1,40}".prop_filter("non-empty", |s| !s.is_empty())
}

proptest! {
    /// A tag survives `Tag → TagSyncData → (serde JSON) → Tag` with identical
    /// id, name, and membership.
    // @internal
    #[test]
    fn prop_tag_sync_data_round_trips(
        name in name_strategy(),
        members in prop::collection::vec("[a-z0-9]{1,12}", 0..8),
    ) {
        let mut tag = Tag::new(&name, 123);
        for m in &members {
            tag.add_contact(m);
        }

        let sync = TagSyncData::from_tag(&tag);
        let json = serde_json::to_string(&sync).unwrap();
        let back: TagSyncData = serde_json::from_str(&json).unwrap();
        let restored = back.to_tag();

        prop_assert_eq!(&restored.id, &tag.id);
        prop_assert_eq!(&restored.name, &name);
        prop_assert_eq!(&restored.contact_ids, &tag.contact_ids);
    }
}

// ── Stateful property (CC-13): membership matches a model ──────────────────────

proptest! {
    /// Random add/remove operations across a small (tag × contact) grid keep the
    /// persisted membership exactly equal to an in-memory model set.
    // @internal
    #[test]
    fn prop_tag_membership_matches_model(
        ops in prop::collection::vec((0usize..3, 0usize..3, any::<bool>()), 1..50),
    ) {
        let storage = open_storage();
        let tag_ids: Vec<String> = (0..3)
            .map(|i| storage.create_tag(&format!("t{i}")).unwrap().id)
            .collect();
        let contacts = ["c0", "c1", "c2"];

        let mut model: BTreeSet<(usize, usize)> = BTreeSet::new();
        for (t, c, add) in ops {
            if add {
                storage.add_to_tag(&tag_ids[t], contacts[c]).unwrap();
                model.insert((t, c));
            } else {
                storage.remove_from_tag(&tag_ids[t], contacts[c]).unwrap();
                model.remove(&(t, c));
            }
        }

        for (t, tid) in tag_ids.iter().enumerate() {
            let tag = storage.get_tag(tid).unwrap().unwrap();
            for (c, cid) in contacts.iter().enumerate() {
                prop_assert_eq!(tag.contains(cid), model.contains(&(t, c)));
            }
        }
    }
}
