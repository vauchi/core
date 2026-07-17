// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for visibility re-propagation.
//!
//! Tests that changing visibility rules via set_field_*_and_repropagate()
//! triggers a new card update to the affected contact.

use vauchi_core::{
    Contact, ContactCard, ContactField, FieldType, Identity, SymmetricKey, Vauchi, VauchiError,
    api::sync::DeviceSyncOrchestrator,
    crypto::SigningKeyPair,
    exchange::X3DHKeyPair,
    identity::{DeviceInfo, DeviceRegistry},
    sync::SyncItem,
};

fn create_test_vauchi() -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    wb
}

fn add_contact_with_ratchet(wb: &Vauchi, name: &str) -> String {
    let identity = Identity::create(name, 0);
    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        shared.clone(),
        0,
    );
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Set up ratchet as initiator so repropagate can encrypt
    let their_dh = X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(&contact_id, &shared, *their_dh.public_key())
        .unwrap();

    contact_id
}

/// Hex id of the own-card field with the given label. Group visibility is
/// keyed by field **id** in production (`screens.rs` GroupDetail factory:
/// `g.is_field_visible(f.id())`), not the label.
fn own_field_id(wb: &Vauchi, label: &str) -> String {
    wb.own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .find(|f| f.label() == label)
        .unwrap()
        .id()
        .to_string()
}

// @scenario: visibility_control :: Revoking visibility sends update to contact
#[test]
fn test_set_field_private_queues_update() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    let pending_before = wb.storage().pending().get_pending_updates(&bob_id).unwrap();
    assert!(
        pending_before.is_empty(),
        "No pending updates before visibility change"
    );

    wb.set_field_private_and_repropagate(&bob_id, "work")
        .unwrap();

    let pending_after = wb.storage().pending().get_pending_updates(&bob_id).unwrap();
    assert!(
        !pending_after.is_empty(),
        "Should queue re-propagation update after visibility change"
    );
}

// @scenario: visibility_control :: Granting visibility sends update to contact
#[test]
fn test_set_field_public_queues_update() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+1234567890",
        0,
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    // Set field public (it's public by default, but this should still trigger repropagate)
    wb.set_field_public_and_repropagate(&bob_id, "mobile")
        .unwrap();

    let pending = wb.storage().pending().get_pending_updates(&bob_id).unwrap();
    assert!(
        !pending.is_empty(),
        "Setting field public should queue a re-propagation update"
    );
}

// @scenario: visibility_control :: Revoking visibility sends update to contact
#[test]
fn test_repropagate_skips_no_ratchet() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ))
    .unwrap();

    let identity = Identity::create("Carol", 0);
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new("Carol"),
        SymmetricKey::generate(),
        0,
    );
    let carol_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Should succeed without error (silently skips)
    let result = wb.set_field_private_and_repropagate(&carol_id, "work");
    assert!(
        result.is_ok(),
        "Re-propagation should silently skip contacts without ratchet"
    );

    // No pending updates (no ratchet to encrypt with)
    let pending = wb
        .storage()
        .pending()
        .get_pending_updates(&carol_id)
        .unwrap();
    assert!(
        pending.is_empty(),
        "No update should be queued for contact without ratchet"
    );
}

// @scenario: visibility_control :: Revoking visibility sends update to contact
#[test]
fn test_visibility_change_nonexistent_contact() {
    let wb = create_test_vauchi();

    let result = wb.set_field_private_and_repropagate("nonexistent-id", "work");
    assert!(
        matches!(result, Err(VauchiError::ContactNotFound(_))),
        "Should return ContactNotFound for nonexistent contact"
    );
}

// @scenario: visibility_control :: Show a field only to specific contacts
#[test]
fn test_set_field_restricted_queues_update() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "personal",
        "alice@personal.com",
        0,
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    wb.set_field_restricted_and_repropagate(
        &bob_id,
        "personal",
        vec!["allowed-contact-1".to_string()],
    )
    .unwrap();

    let pending = wb.storage().pending().get_pending_updates(&bob_id).unwrap();
    assert!(
        !pending.is_empty(),
        "Restricted visibility change should queue an update"
    );
}

// ============================================================
// ============================================================

// @scenario: visibility_control :: Add contact to group updates their visibility
#[test]
fn test_add_contact_to_label_triggers_repropagate() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");
    let label = wb.create_group("Work").unwrap();
    let work = own_field_id(&wb, "work");

    wb.set_group_field_visibility(label.id(), &work, true)
        .unwrap();

    let pending_before = wb.storage().pending().get_pending_updates(&bob_id).unwrap();
    assert!(pending_before.is_empty());

    wb.add_contact_to_group_and_repropagate(label.id(), &bob_id)
        .unwrap();

    let pending_after = wb.storage().pending().get_pending_updates(&bob_id).unwrap();
    assert!(
        !pending_after.is_empty(),
        "Adding contact to label should queue a re-propagation update"
    );
}

// @scenario: visibility_control :: Remove contact from group updates their visibility
#[test]
fn test_remove_contact_from_label_triggers_repropagate() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");
    let label = wb.create_group("Work").unwrap();
    let work = own_field_id(&wb, "work");
    wb.set_group_field_visibility(label.id(), &work, true)
        .unwrap();

    wb.add_contact_to_group(label.id(), &bob_id).unwrap();
    // While in Work (which grants `work`), Bob can see it.
    assert!(wb.get_effective_field_visibility(&bob_id, &work).unwrap());

    // Make `work` group-only: remove it from the public base so the Work group
    // is its sole grant. ADR-054 D3 — an ungrouped contact falls back to the
    // public base card, so only a group-only field is revoked by leaving the
    // group. A group grant (Layer B) outranks the public base, so Bob still
    // sees `work` right now.
    wb.set_own_field_private(&work).unwrap();
    assert!(
        wb.get_effective_field_visibility(&bob_id, &work).unwrap(),
        "In the Work group, Bob sees `work` despite it not being in the public base"
    );

    wb.remove_contact_from_group_and_repropagate(label.id(), &bob_id)
        .unwrap();

    // Removed from his only group → falls back to the public base, where `work`
    // is private → revoked. (A public-base field would survive removal.)
    assert!(
        !wb.get_effective_field_visibility(&bob_id, &work).unwrap(),
        "Group-only `work` is revoked when Bob leaves his only granting group"
    );
}

// @scenario: visibility_control :: Apply visibility group to a field
#[test]
fn test_set_label_field_visibility_repropagates_to_all_members() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");
    let carol_id = add_contact_with_ratchet(&wb, "Carol");

    let label = wb.create_group("Team").unwrap();
    let work = own_field_id(&wb, "work");
    wb.add_contact_to_group(label.id(), &bob_id).unwrap();
    wb.add_contact_to_group(label.id(), &carol_id).unwrap();

    wb.set_group_field_visibility_and_repropagate(label.id(), &work, true)
        .unwrap();

    let bob_pending = wb.storage().pending().get_pending_updates(&bob_id).unwrap();
    let carol_pending = wb
        .storage()
        .pending()
        .get_pending_updates(&carol_id)
        .unwrap();

    assert!(
        !bob_pending.is_empty(),
        "Bob should receive re-propagation update"
    );
    assert!(
        !carol_pending.is_empty(),
        "Carol should receive re-propagation update"
    );
}

// @scenario: visibility_control :: Hide a field from a specific contact
#[test]
fn test_set_contact_override_triggers_repropagate() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "personal",
        "alice@personal.com",
        0,
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    let pending_before = wb.storage().pending().get_pending_updates(&bob_id).unwrap();
    assert!(pending_before.is_empty());

    wb.set_contact_visibility_override_and_repropagate(&bob_id, "personal", false)
        .unwrap();

    let pending_after = wb.storage().pending().get_pending_updates(&bob_id).unwrap();
    assert!(
        !pending_after.is_empty(),
        "Per-contact override should queue a re-propagation update"
    );
}

/// A one-contact visibility change must update that contact immediately and
/// retain the linked-device journal entry, so a sibling cannot later compute
/// the field as hidden and revoke it.
// @scenario: visibility_control :: Granting visibility sends update to contact
// @scenario: device_management :: Visibility grants converge across linked devices
#[test]
fn test_contact_override_and_repropagate_journals_linked_device_grant() {
    let wb = create_test_vauchi();
    const SEED: [u8; 32] = [41u8; 32];
    let signing = SigningKeyPair::from_seed(&SEED);
    let mut registry = DeviceRegistry::new(
        DeviceInfo::derive(&SEED, 0, "phone".into(), 0).to_registered(&SEED),
        &signing,
    );
    let tablet = DeviceInfo::derive(&SEED, 1, "tablet".into(), 0);
    let tablet_id = *tablet.device_id();
    registry
        .add_device_unsigned(tablet.to_registered(&SEED))
        .unwrap();
    wb.storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "personal",
        "alice@personal.com",
        0,
    ))
    .unwrap();
    let field_id = own_field_id(&wb, "personal");
    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    wb.set_contact_visibility_override_and_repropagate(&bob_id, &field_id, true)
        .unwrap();

    let orchestrator = DeviceSyncOrchestrator::load(
        wb.storage(),
        wb.identity().unwrap().create_device_info(0),
        registry,
    )
    .unwrap();
    assert!(
        orchestrator
            .pending_for_device(&tablet_id)
            .iter()
            .any(|item| matches!(
                item,
                SyncItem::VisibilityChanged {
                    contact_id,
                    field_id: synced_field_id,
                    is_visible: true,
                    ..
                } if contact_id == &bob_id && synced_field_id == &field_id
            ))
    );
}

// @scenario: sync_updates :: A secondary-device card change reaches contacts
// @internal
#[test]
fn synced_field_and_visibility_grant_repropagate_through_a_sibling_ratchet() {
    let wb = create_test_vauchi();
    let bob_id = add_contact_with_ratchet(&wb, "Bob");
    let field = ContactField::new(FieldType::Phone, "secondary-phone", "+12025550100", 10);
    let field_id = field.id().to_string();

    let applied = wb
        .apply_sync_items(vec![
            SyncItem::CardFieldSynced {
                field,
                field_visibility: None,
                timestamp: 10,
            },
            SyncItem::VisibilityChanged {
                contact_id: bob_id.clone(),
                field_id,
                is_visible: true,
                timestamp: 11,
            },
        ])
        .unwrap();
    assert_eq!(applied, 2);

    wb.run_owed_repropagation().unwrap();

    assert!(
        !wb.storage()
            .pending()
            .get_pending_updates(&bob_id)
            .unwrap()
            .is_empty(),
        "a sibling's received field and visibility grant must enqueue a peer update"
    );
}

// @scenario: visibility_control :: Granting visibility sends update to contact
// @scenario: visibility_control :: Hide a field from a specific contact
#[test]
fn test_repropagate_uses_effective_visibility() {
    let wb = create_test_vauchi();

    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ))
    .unwrap();
    wb.add_own_field(ContactField::new(
        FieldType::Phone,
        "personal-phone",
        "+1234567890",
        0,
    ))
    .unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");

    let label = wb.create_group("Work").unwrap();
    let work = own_field_id(&wb, "work");
    let phone = own_field_id(&wb, "personal-phone");
    wb.set_group_field_visibility(label.id(), &work, true)
        .unwrap();

    wb.add_contact_to_group(label.id(), &bob_id).unwrap();
    wb.set_contact_visibility_override(&bob_id, &phone, false)
        .unwrap();

    assert!(
        wb.get_effective_field_visibility(&bob_id, &work).unwrap(),
        "Work field should be visible via label"
    );
    assert!(
        !wb.get_effective_field_visibility(&bob_id, &phone).unwrap(),
        "Personal phone should be hidden via override"
    );

    // Re-propagate using the new label-aware method
    wb.set_field_public_and_repropagate(&bob_id, &work).unwrap();

    // Should have queued an update (the re-propagation uses effective visibility)
    let pending = wb.storage().pending().get_pending_updates(&bob_id).unwrap();
    assert!(
        !pending.is_empty(),
        "Re-propagation should queue update using effective visibility"
    );
}

// ── Default-closed in groups mode (2026-06-08-sync-card-update-not-group-filtered, A) ──

/// Own card with `work` + `personal`, a `Work` group exposing only `work`,
/// Bob in `Work`. Returns the engine and Bob's id.
fn vauchi_with_work_group_and_bob() -> (Vauchi, String) {
    let wb = create_test_vauchi();
    wb.add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    wb.add_own_field(ContactField::new(
        FieldType::Phone,
        "personal",
        "+12025550123",
        0,
    ))
    .unwrap();
    let bob_id = add_contact_with_ratchet(&wb, "Bob");
    let work = wb.create_group("Work").unwrap();
    wb.set_group_field_visibility(work.id(), "work", true)
        .unwrap();
    wb.add_contact_to_group(work.id(), &bob_id).unwrap();
    (wb, bob_id)
}

// @internal
#[test]
fn groups_mode_field_in_no_group_is_hidden() {
    // `personal` is in no group → default-closed in groups mode → hidden.
    let (wb, bob_id) = vauchi_with_work_group_and_bob();
    assert!(
        wb.get_effective_field_visibility(&bob_id, "work").unwrap(),
        "Work group grants `work` → visible"
    );
    assert!(
        !wb.get_effective_field_visibility(&bob_id, "personal")
            .unwrap(),
        "`personal` is in no group → must be hidden (default-closed in groups mode)"
    );
}

// @internal
#[test]
fn ungrouped_contact_sees_only_visible_toggled_unassigned_fields() {
    // Field-centric model (2026-07-10): a contact in zero groups sees an
    // unassigned field only once its Visible toggle is set; a group-assigned
    // field stays closed to them regardless of any toggle.
    let (wb, _bob_id) = vauchi_with_work_group_and_bob();
    let carol_id = add_contact_with_ratchet(&wb, "Carol");

    // `personal` is unassigned and untoggled → hidden by default.
    assert!(
        !wb.get_effective_field_visibility(&carol_id, "personal")
            .unwrap(),
        "an untoggled unassigned field is hidden from the ungrouped contact"
    );

    // The Visible toggle shows it to the ungrouped contact.
    wb.set_own_field_public("personal").unwrap();
    assert!(
        wb.get_effective_field_visibility(&carol_id, "personal")
            .unwrap(),
        "the Visible toggle shows the unassigned field"
    );

    // `work` is group-assigned → closed to a non-member even when toggled.
    wb.set_own_field_public("work").unwrap();
    assert!(
        !wb.get_effective_field_visibility(&carol_id, "work")
            .unwrap(),
        "a group-assigned field stays closed to a contact outside the group"
    );
}

// @internal
#[test]
fn per_contact_override_grants_even_in_groups_mode() {
    // Override is Layer C — highest precedence — so it can grant a field that
    // no group grants, even in groups mode.
    let (wb, _bob_id) = vauchi_with_work_group_and_bob();
    let dave_id = add_contact_with_ratchet(&wb, "Dave");
    wb.set_contact_visibility_override(&dave_id, "personal", true)
        .unwrap();
    assert!(
        wb.get_effective_field_visibility(&dave_id, "personal")
            .unwrap(),
        "Per-contact override grants `personal` despite no group granting it"
    );
}

// @internal
#[test]
fn set_field_private_for_a_grouped_contact_hides_via_override() {
    // ADR-054 review (2026-06-14): for a *grouped* contact, set_field_private
    // routes to a per-contact override (Layer C), which beats the group grant —
    // so the field is hidden even though the group grants it. (A Layer-A write
    // would be silently ignored for a grouped contact.)
    let (wb, bob_id) = vauchi_with_work_group_and_bob();
    assert!(
        wb.get_effective_field_visibility(&bob_id, "work").unwrap(),
        "precondition: the Work group grants `work` to grouped Bob"
    );

    wb.set_field_private_and_repropagate(&bob_id, "work")
        .unwrap();

    assert!(
        !wb.get_effective_field_visibility(&bob_id, "work").unwrap(),
        "set_field_private on a grouped contact hides `work` via an override, beating the group grant"
    );
}

// @internal
#[test]
fn no_groups_mode_defaults_hidden_until_toggled() {
    // With NO groups at all, an untoggled field is hidden (field-centric
    // default) and the Visible toggle is the single control that shows it.
    let wb = create_test_vauchi();
    wb.add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    let work = own_field_id(&wb, "work");
    let bob_id = add_contact_with_ratchet(&wb, "Bob");
    assert!(
        !wb.get_effective_field_visibility(&bob_id, &work).unwrap(),
        "No-groups mode → an untoggled field is hidden by default"
    );
    wb.set_own_field_public(&work).unwrap();
    assert!(
        wb.get_effective_field_visibility(&bob_id, &work).unwrap(),
        "No-groups mode → the Visible toggle shows the field"
    );
}

// @scenario: visibility_control :: Deleting a group revokes its fields from members
#[test]
fn test_delete_group_revokes_group_only_field_from_members() {
    let wb = create_test_vauchi();
    wb.add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    let work = own_field_id(&wb, "work");
    // Public base hidden → `work` reaches a grouped contact only via the group.
    wb.set_own_field_private(&work).unwrap();

    let bob_id = add_contact_with_ratchet(&wb, "Bob");
    let label = wb.create_group("Team").unwrap();
    wb.add_contact_to_group(label.id(), &bob_id).unwrap();
    wb.set_group_field_visibility(label.id(), &work, true)
        .unwrap();
    // Baseline: Bob currently receives `work` via the group grant.
    wb.initialize_sent_baseline(&bob_id).unwrap();
    assert!(
        wb.get_effective_field_visibility(&bob_id, &work).unwrap(),
        "precondition: the group grant makes `work` visible to Bob"
    );
    let before = wb
        .storage()
        .pending()
        .get_pending_updates(&bob_id)
        .unwrap()
        .len();

    wb.delete_group_and_repropagate(label.id()).unwrap();

    assert!(
        !wb.get_effective_field_visibility(&bob_id, &work).unwrap(),
        "deleting the only granting group hides `work` (public base is private)"
    );
    let after_sent = wb
        .storage()
        .contacts()
        .load_last_sent_visible_fields(&bob_id)
        .unwrap()
        .unwrap_or_default();
    assert!(
        !after_sent.contains(&work),
        "the revocation drops `work` from the set last sent to Bob (revoked on the wire)"
    );
    let after = wb
        .storage()
        .pending()
        .get_pending_updates(&bob_id)
        .unwrap()
        .len();
    assert!(
        after > before,
        "deleting the group must queue a revocation update to the former member"
    );
}
