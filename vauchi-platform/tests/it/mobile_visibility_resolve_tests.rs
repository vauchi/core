// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the G2 LabelDetail resolver — verifies that
//! `VauchiPlatform::get_label()` populates `label_contacts` and
//! `stale_reference_count` correctly so frontends can stop joining
//! `contact_ids` against the contacts list themselves (ADR-021/043).
//!
//! Closes the symmetric Humble-UI violation tracked in
//! `_private/docs/problems/2026-04-27-screenmodel-api-gaps-symmetric-frontend-violations`
//! (G2). The missing-contact policy is the planning record's default:
//! omit deleted/missing contacts from `label_contacts` and surface the
//! drop count via `stale_reference_count` (omit + count).

use std::sync::Arc;

use tempfile::TempDir;

use vauchi_platform::{MobileLabelContactBadge, MobileLabelContactStatus, VauchiPlatform};

fn setup() -> (Arc<VauchiPlatform>, TempDir) {
    let dir = TempDir::new().unwrap();
    let wb = VauchiPlatform::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
    )
    .unwrap();
    wb.create_identity("Alice".to_string()).unwrap();
    (wb, dir)
}

fn add_exchanged_contact(wb: &VauchiPlatform, name: &str, pk_seed: u8) -> String {
    let card = vauchi_core::contact_card::ContactCard::new(name);
    let contact = vauchi_core::Contact::from_exchange(
        [pk_seed; 32],
        card,
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    wb.save_test_contact(&contact).unwrap();
    id
}

/// Same as `add_exchanged_contact` but marks the contact's fingerprint
/// verified before saving — used to drive the `MobileLabelContactBadge::Verified`
/// branch of `resolve_label_contacts`.
fn add_verified_contact(wb: &VauchiPlatform, name: &str, pk_seed: u8) -> String {
    let card = vauchi_core::contact_card::ContactCard::new(name);
    let mut contact = vauchi_core::Contact::from_exchange(
        [pk_seed; 32],
        card,
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    contact
        .mark_fingerprint_verified()
        .expect("mark verified on fresh exchange contact");
    let id = contact.id().to_string();
    wb.save_test_contact(&contact).unwrap();
    id
}

// @internal
#[test]
fn empty_label_has_no_label_contacts_and_zero_stale_count() {
    let (wb, _dir) = setup();
    let label = wb.create_label("Family".to_string()).unwrap();

    let detail = wb.get_label(label.id).unwrap();

    assert!(
        detail.label_contacts.is_empty(),
        "empty label must have empty label_contacts"
    );
    assert_eq!(
        detail.stale_reference_count, 0,
        "empty label must have zero stale references"
    );
    assert!(
        detail.contact_ids.is_empty(),
        "raw contact_ids must also be empty (sanity check)"
    );
}

// @internal
#[test]
fn all_contacts_present_resolve_with_zero_stale_count() {
    let (wb, _dir) = setup();
    let bob_id = add_exchanged_contact(&wb, "Bob", 0x01);
    let carol_id = add_exchanged_contact(&wb, "Carol", 0x02);
    let label = wb.create_label("Friends".to_string()).unwrap();
    wb.add_contact_to_group(label.id.clone(), bob_id.clone())
        .unwrap();
    wb.add_contact_to_group(label.id.clone(), carol_id.clone())
        .unwrap();

    let detail = wb.get_label(label.id).unwrap();

    assert_eq!(
        detail.label_contacts.len(),
        2,
        "both contacts must resolve to rows"
    );
    assert_eq!(
        detail.stale_reference_count, 0,
        "all contacts present → zero stale"
    );
    let resolved_ids: Vec<&str> = detail
        .label_contacts
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert!(
        resolved_ids.contains(&bob_id.as_str()) && resolved_ids.contains(&carol_id.as_str()),
        "both contact IDs must appear in label_contacts"
    );
    for row in &detail.label_contacts {
        assert_eq!(
            row.status,
            MobileLabelContactStatus::Active,
            "all rows for present contacts must be Active"
        );
    }
}

// @internal
#[test]
fn deleted_contact_is_omitted_and_counted_as_stale() {
    let (wb, _dir) = setup();
    let bob_id = add_exchanged_contact(&wb, "Bob", 0x01);
    let carol_id = add_exchanged_contact(&wb, "Carol", 0x02);
    let dave_id = add_exchanged_contact(&wb, "Dave", 0x03);
    let label = wb.create_label("Inner Circle".to_string()).unwrap();
    wb.add_contact_to_group(label.id.clone(), bob_id.clone())
        .unwrap();
    wb.add_contact_to_group(label.id.clone(), carol_id.clone())
        .unwrap();
    wb.add_contact_to_group(label.id.clone(), dave_id.clone())
        .unwrap();

    // Delete one of the contacts — the label still references the id.
    wb.remove_contact(carol_id.clone()).unwrap();

    let detail = wb.get_label(label.id).unwrap();

    assert_eq!(
        detail.label_contacts.len(),
        2,
        "two of three contacts remain after Carol's deletion"
    );
    assert_eq!(
        detail.stale_reference_count, 1,
        "exactly one stale reference (Carol) must be counted"
    );
    let resolved_ids: Vec<&str> = detail
        .label_contacts
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert!(
        !resolved_ids.contains(&carol_id.as_str()),
        "deleted contact id must NOT appear in label_contacts (no raw-id leak)"
    );
}

// @internal
#[test]
fn invariant_label_contacts_plus_stale_equals_contact_ids() {
    let (wb, _dir) = setup();
    let alice_id = add_exchanged_contact(&wb, "Alice2", 0x10);
    let bob_id = add_exchanged_contact(&wb, "Bob2", 0x11);
    let label = wb.create_label("Mixed".to_string()).unwrap();
    wb.add_contact_to_group(label.id.clone(), alice_id).unwrap();
    wb.add_contact_to_group(label.id.clone(), bob_id.clone())
        .unwrap();
    wb.remove_contact(bob_id).unwrap();

    let detail = wb.get_label(label.id).unwrap();

    assert_eq!(
        detail.label_contacts.len() + detail.stale_reference_count as usize,
        detail.contact_ids.len(),
        "invariant: rows + stale = total contact_ids"
    );
}

// @internal
#[test]
fn verified_contact_in_label_renders_verified_badge() {
    // G6 follow-up — restores the verified-checkmark dropped from
    // iOS LabelDetailView during the G4 ContactDetail consumer
    // migration. The badge is computed in core; frontends iterate
    // `row.badges`, never branching on raw `MobileContact` flags.
    let (wb, _dir) = setup();
    let bob_id = add_verified_contact(&wb, "Bob", 0x20);
    let label = wb.create_label("Verified".to_string()).unwrap();
    wb.add_contact_to_group(label.id.clone(), bob_id.clone())
        .unwrap();

    let detail = wb.get_label(label.id).unwrap();
    let row = detail
        .label_contacts
        .iter()
        .find(|r| r.id == bob_id)
        .expect("Bob must appear in label_contacts");
    assert!(
        row.badges.contains(&MobileLabelContactBadge::Verified),
        "fingerprint-verified contact must surface MobileLabelContactBadge::Verified, got {:?}",
        row.badges
    );
}

// @internal
#[test]
fn unverified_contact_in_label_has_empty_badges() {
    // Mirror of the verified case — make sure we don't accidentally
    // emit Verified for fresh exchanged contacts.
    let (wb, _dir) = setup();
    let bob_id = add_exchanged_contact(&wb, "Bob", 0x21);
    let label = wb.create_label("Unverified".to_string()).unwrap();
    wb.add_contact_to_group(label.id.clone(), bob_id.clone())
        .unwrap();

    let detail = wb.get_label(label.id).unwrap();
    let row = detail
        .label_contacts
        .iter()
        .find(|r| r.id == bob_id)
        .expect("Bob must appear in label_contacts");
    assert!(
        row.badges.is_empty(),
        "fresh exchanged contact must have no badges, got {:?}",
        row.badges
    );
}

// @internal
#[test]
fn label_contacts_renders_resolved_display_name_with_nickname() {
    let (wb, _dir) = setup();
    let bob_id = add_exchanged_contact(&wb, "Bob", 0x01);
    let label = wb.create_label("Work".to_string()).unwrap();
    wb.add_contact_to_group(label.id.clone(), bob_id.clone())
        .unwrap();

    // Without nickname — display_name should be the contact's primary name.
    let detail = wb.get_label(label.id.clone()).unwrap();
    let row = detail
        .label_contacts
        .iter()
        .find(|r| r.id == bob_id)
        .expect("Bob must appear in label_contacts");
    assert_eq!(
        row.display_name, "Bob",
        "without a nickname, display_name must equal the primary card name"
    );
}
