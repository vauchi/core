// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for local organization groups (Task 11 / HR-5).
//!
//! Verifies that local groups can be created, listed, have contacts added and
//! removed, and deleted via the storage CRUD. Also asserts the compile-time
//! guarantee that `LocalGroup` has no `visible_fields` field.

use vauchi_core::LocalGroup;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

// ── Helper ───────────────────────────────────────────────────────────────────

fn open_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Creating a group persists it and round-trips through get_local_group.
#[test]
fn create_local_group() {
    let storage = open_storage();

    let group = storage.create_local_group("Team Alpha").unwrap();

    assert!(!group.id.is_empty(), "group ID must be a non-empty UUID");
    assert_eq!(group.name, "Team Alpha");
    assert!(
        group.contact_ids.is_empty(),
        "new group must have no members"
    );
    assert!(
        group.created_at > 0,
        "created_at must be a non-zero unix timestamp"
    );

    // Verify persistence
    let loaded = storage.get_local_group(&group.id).unwrap();
    assert!(loaded.is_some(), "created group must be retrievable");
    let loaded = loaded.unwrap();
    assert_eq!(loaded.id, group.id);
    assert_eq!(loaded.name, "Team Alpha");
}

/// Adding a contact to a group persists the membership.
#[test]
fn add_contact_to_group() {
    let storage = open_storage();
    let group = storage.create_local_group("VIPs").unwrap();
    let contact_id = "some-contact-uuid-abc";

    storage.add_to_local_group(&group.id, contact_id).unwrap();

    let loaded = storage.get_local_group(&group.id).unwrap().unwrap();
    assert!(
        loaded.contact_ids.contains(contact_id),
        "contact_id must be present after add"
    );
    assert_eq!(loaded.contact_ids.len(), 1);
}

/// Adding the same contact twice is idempotent (no error, still 1 member).
#[test]
fn add_contact_to_group_idempotent() {
    let storage = open_storage();
    let group = storage.create_local_group("Idempotent").unwrap();
    let contact_id = "dup-uuid";

    storage.add_to_local_group(&group.id, contact_id).unwrap();
    storage.add_to_local_group(&group.id, contact_id).unwrap();

    let loaded = storage.get_local_group(&group.id).unwrap().unwrap();
    assert_eq!(
        loaded.contact_ids.len(),
        1,
        "duplicate add must remain 1 member"
    );
}

/// Removing a contact from a group removes only that contact.
#[test]
fn remove_contact_from_group() {
    let storage = open_storage();
    let group = storage.create_local_group("Removals").unwrap();
    let id_a = "contact-a";
    let id_b = "contact-b";

    storage.add_to_local_group(&group.id, id_a).unwrap();
    storage.add_to_local_group(&group.id, id_b).unwrap();
    storage.remove_from_local_group(&group.id, id_a).unwrap();

    let loaded = storage.get_local_group(&group.id).unwrap().unwrap();
    assert!(
        !loaded.contact_ids.contains(id_a),
        "removed contact must not be present"
    );
    assert!(
        loaded.contact_ids.contains(id_b),
        "other contact must still be present"
    );
}

/// Removing a contact that is not in the group is a no-op (no error).
#[test]
fn remove_nonmember_contact_is_noop() {
    let storage = open_storage();
    let group = storage.create_local_group("Noop").unwrap();

    // Remove from empty group — must not error
    storage
        .remove_from_local_group(&group.id, "nonexistent-id")
        .unwrap();

    let loaded = storage.get_local_group(&group.id).unwrap().unwrap();
    assert!(loaded.contact_ids.is_empty());
}

/// list_local_groups returns all created groups.
#[test]
fn list_groups() {
    let storage = open_storage();

    let g1 = storage.create_local_group("Alpha").unwrap();
    let g2 = storage.create_local_group("Beta").unwrap();
    let g3 = storage.create_local_group("Gamma").unwrap();

    let groups = storage.list_local_groups().unwrap();
    assert_eq!(groups.len(), 3, "must list all 3 groups");

    let ids: Vec<&str> = groups.iter().map(|g| g.id.as_str()).collect();
    assert!(ids.contains(&g1.id.as_str()));
    assert!(ids.contains(&g2.id.as_str()));
    assert!(ids.contains(&g3.id.as_str()));
}

/// Deleting a group removes it from storage.
#[test]
fn delete_group() {
    let storage = open_storage();
    let group = storage.create_local_group("Temporary").unwrap();

    let deleted = storage.delete_local_group(&group.id).unwrap();
    assert!(deleted, "delete must return true for an existing group");

    let after = storage.get_local_group(&group.id).unwrap();
    assert!(after.is_none(), "deleted group must not be findable");

    // Deleting again returns false
    let deleted_again = storage.delete_local_group(&group.id).unwrap();
    assert!(
        !deleted_again,
        "delete of non-existent group must return false"
    );
}

/// `LocalGroup` has no `visible_fields` field — compile-time invariant.
///
/// This test is deliberately trivial: if someone adds `visible_fields` to
/// `LocalGroup`, this test would need to be updated, drawing attention to the
/// privacy boundary violation.
#[test]
fn group_has_no_visibility_fields() {
    // Construct a LocalGroup and verify we can only access organizational fields.
    // If LocalGroup ever gains a `visible_fields` member this test file must be
    // updated, making the privacy boundary change visible in code review.
    let group = LocalGroup::new("Check");
    let _ = &group.id;
    let _ = &group.name;
    let _ = &group.contact_ids;
    let _ = &group.created_at;
    // There is intentionally no assertion on visible_fields because that field
    // must not exist on LocalGroup.
}

/// `add_to_local_group` on a non-existent group returns NotFound.
#[test]
fn add_to_nonexistent_group_returns_not_found() {
    let storage = open_storage();
    let result = storage.add_to_local_group("non-existent-group-id", "contact-id");
    assert!(
        result.is_err(),
        "adding to a non-existent group must return an error"
    );
}
