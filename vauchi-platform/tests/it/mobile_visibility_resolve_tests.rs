// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the G2 LabelDetail resolver — verifies that the
//! `DomainCommand::GetLabel` handler populates `label_contacts` and
//! `stale_reference_count` correctly so frontends can stop joining
//! `contact_ids` against the contacts list themselves (ADR-021/043).
//!
//! Closes the symmetric Humble-UI violation tracked in
//! `_private/docs/problems/2026-04-27-screenmodel-api-gaps-symmetric-frontend-violations`
//! (G2). The missing-contact policy is the planning record's default:
//! omit deleted/missing contacts from `label_contacts` and surface the
//! drop count via `stale_reference_count` (omit + count).
//!
//! Slice 32b (2026-05-16) retired the legacy `VauchiPlatform`
//! visibility methods; label CRUD now routes through
//! `PlatformAppEngine::dispatch_domain_command`. Identity creation
//! drives through PAE's onboarding `handle_action_json` flow.
//! Contact injection uses the `save_test_contact` test helper
//! (mirrors the one on `VauchiPlatform`); contact deletion uses
//! `DomainCommand::RemoveContact`. A single `PlatformAppEngine`
//! instance owns writes and reads, so the underlying `Vauchi`'s
//! in-memory state stays coherent without the dual-app race the
//! sibling-`VauchiPlatform` pattern would introduce.

use std::sync::Arc;

use tempfile::TempDir;

use vauchi_platform::{
    DomainCommand, DomainCommandResult, MobileLabelContactBadge, MobileLabelContactStatus,
    MobileVisibilityLabel, MobileVisibilityLabelDetail, PlatformAppEngine,
    PlatformAppEngineTestHelpers,
};

fn setup() -> (Arc<PlatformAppEngine>, TempDir) {
    let dir = TempDir::new().unwrap();
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
        key.as_bytes().to_vec(),
    )
    .expect("create PlatformAppEngine");
    drive_onboarding(&engine);
    (engine, dir)
}

/// Drive the onboarding flow to create the identity. Mirrors the
/// pattern in `platform_app_engine_domain_command_tests.rs`.
fn drive_onboarding(engine: &PlatformAppEngine) {
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "create_new"}}"#.into())
        .expect("create_new");
    engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "display_name", "value": "Alice"}}"#.into(),
        )
        .expect("display_name");
    for _ in 0..3 {
        engine
            .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
            .expect("continue");
    }
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "start_app"}}"#.into())
        .expect("start_app");
}

fn create_label(engine: &PlatformAppEngine, name: &str) -> MobileVisibilityLabel {
    match engine
        .dispatch_domain_command(DomainCommand::CreateLabel { name: name.into() })
        .expect("CreateLabel dispatch")
    {
        DomainCommandResult::Label { label } => label,
        other => panic!("expected DomainCommandResult::Label, got {other:?}"),
    }
}

fn add_contact_to_group(engine: &PlatformAppEngine, label_id: &str, contact_id: &str) {
    match engine
        .dispatch_domain_command(DomainCommand::AddContactToGroup {
            label_id: label_id.into(),
            contact_id: contact_id.into(),
        })
        .expect("AddContactToGroup dispatch")
    {
        DomainCommandResult::Unit => {}
        other => panic!("expected DomainCommandResult::Unit, got {other:?}"),
    }
}

fn get_label(engine: &PlatformAppEngine, label_id: &str) -> MobileVisibilityLabelDetail {
    match engine
        .dispatch_domain_command(DomainCommand::GetLabel {
            label_id: label_id.into(),
        })
        .expect("GetLabel dispatch")
    {
        DomainCommandResult::LabelDetail { detail } => detail,
        other => panic!("expected DomainCommandResult::LabelDetail, got {other:?}"),
    }
}

fn remove_contact(engine: &PlatformAppEngine, contact_id: &str) {
    match engine
        .dispatch_domain_command(DomainCommand::RemoveContact {
            id: contact_id.into(),
        })
        .expect("RemoveContact dispatch")
    {
        DomainCommandResult::Bool { value } => assert!(
            value,
            "RemoveContact must report removed=true for an existing contact"
        ),
        other => panic!("expected DomainCommandResult::Bool, got {other:?}"),
    }
}

fn add_exchanged_contact(engine: &PlatformAppEngine, name: &str, pk_seed: u8) -> String {
    let card = vauchi_core::contact_card::ContactCard::new(name);
    let contact = vauchi_core::Contact::from_exchange(
        [pk_seed; 32],
        card,
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    engine.save_test_contact(&contact).unwrap();
    id
}

/// Same as `add_exchanged_contact` but marks the contact's fingerprint
/// verified before saving — used to drive the `MobileLabelContactBadge::Verified`
/// branch of `resolve_label_contacts`.
fn add_verified_contact(engine: &PlatformAppEngine, name: &str, pk_seed: u8) -> String {
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
    engine.save_test_contact(&contact).unwrap();
    id
}

// @internal
#[test]
fn empty_label_has_no_label_contacts_and_zero_stale_count() {
    let (engine, _dir) = setup();
    let label = create_label(&engine, "Family");

    let detail = get_label(&engine, &label.id);

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
    let (engine, _dir) = setup();
    let bob_id = add_exchanged_contact(&engine, "Bob", 0x01);
    let carol_id = add_exchanged_contact(&engine, "Carol", 0x02);
    let label = create_label(&engine, "Friends");
    add_contact_to_group(&engine, &label.id, &bob_id);
    add_contact_to_group(&engine, &label.id, &carol_id);

    let detail = get_label(&engine, &label.id);

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
    let (engine, _dir) = setup();
    let bob_id = add_exchanged_contact(&engine, "Bob", 0x01);
    let carol_id = add_exchanged_contact(&engine, "Carol", 0x02);
    let dave_id = add_exchanged_contact(&engine, "Dave", 0x03);
    let label = create_label(&engine, "Inner Circle");
    add_contact_to_group(&engine, &label.id, &bob_id);
    add_contact_to_group(&engine, &label.id, &carol_id);
    add_contact_to_group(&engine, &label.id, &dave_id);

    // Delete one of the contacts — the label still references the id.
    remove_contact(&engine, &carol_id);

    let detail = get_label(&engine, &label.id);

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
    let (engine, _dir) = setup();
    let alice_id = add_exchanged_contact(&engine, "Alice2", 0x10);
    let bob_id = add_exchanged_contact(&engine, "Bob2", 0x11);
    let label = create_label(&engine, "Mixed");
    add_contact_to_group(&engine, &label.id, &alice_id);
    add_contact_to_group(&engine, &label.id, &bob_id);
    remove_contact(&engine, &bob_id);

    let detail = get_label(&engine, &label.id);

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
    let (engine, _dir) = setup();
    let bob_id = add_verified_contact(&engine, "Bob", 0x20);
    let label = create_label(&engine, "Verified");
    add_contact_to_group(&engine, &label.id, &bob_id);

    let detail = get_label(&engine, &label.id);
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
    let (engine, _dir) = setup();
    let bob_id = add_exchanged_contact(&engine, "Bob", 0x21);
    let label = create_label(&engine, "Unverified");
    add_contact_to_group(&engine, &label.id, &bob_id);

    let detail = get_label(&engine, &label.id);
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
    let (engine, _dir) = setup();
    let bob_id = add_exchanged_contact(&engine, "Bob", 0x01);
    let label = create_label(&engine, "Work");
    add_contact_to_group(&engine, &label.id, &bob_id);

    // Without nickname — display_name should be the contact's primary name.
    let detail = get_label(&engine, &label.id);
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
