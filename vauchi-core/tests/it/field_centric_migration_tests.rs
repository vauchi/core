// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! One-time grandfathering migration for the field-centric visibility model
//! (`2026-07-05-ungrouped-contacts-default-open`, Phase 2): unruled entries
//! materialize to explicit `Everyone` (preserving what contacts observably
//! see), explicit rules are never touched, repropagation is armed only when
//! groups exist, and fresh installs are marked at identity creation so the
//! sweep never runs for them.

use vauchi_core::{Contact, ContactCard, ContactField, FieldType, SymmetricKey, Vauchi};

/// Identity + two unruled fields + one ungrouped exchanged contact, with the
/// migration marker cleared — simulating an install that predates the
/// field-centric model (`create_identity` sets the marker on fresh installs).
fn pre_migration_world() -> (Vauchi, String, String, String) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Owner").unwrap();
    let email = ContactField::new(FieldType::Email, "Work", "o@co.example", 0);
    let phone = ContactField::new(FieldType::Phone, "Mobile", "+15550100", 0);
    let email_id = email.id().to_string();
    let phone_id = phone.id().to_string();
    wb.add_own_field(email).unwrap();
    wb.add_own_field(phone).unwrap();

    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange([7u8; 32], ContactCard::new("Bob"), shared.clone(), 0);
    let bob = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    let their_dh = vauchi_core::exchange::X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(&bob, &shared, *their_dh.public_key())
        .unwrap();

    let mut flags = wb.load_settings_flags().unwrap();
    flags.field_centric_visibility_migrated = false;
    wb.save_settings_flags(&flags).unwrap();

    (wb, email_id, phone_id, bob)
}

// @internal
#[test]
fn migration_materializes_unruled_fields_to_visible() {
    let (wb, email_id, phone_id, bob) = pre_migration_world();
    assert!(
        !wb.get_effective_field_visibility(&bob, &email_id).unwrap(),
        "precondition: unruled entry is hidden before migration"
    );

    let ran = wb.migrate_field_centric_visibility().unwrap();
    assert!(ran, "migration must run when the marker is cleared");

    for fid in [&email_id, &phone_id] {
        assert!(
            wb.get_effective_field_visibility(&bob, fid).unwrap(),
            "grandfathered entry stays visible to the ungrouped contact"
        );
    }
    let card = wb.own_card().unwrap().unwrap();
    assert!(
        card.field_visibility().is_explicitly_everyone(&email_id),
        "materialization writes an explicit Everyone rule"
    );
}

// @internal
#[test]
fn migration_never_touches_explicit_rules() {
    let (wb, email_id, phone_id, bob) = pre_migration_world();
    wb.set_own_field_private(&email_id).unwrap();

    wb.migrate_field_centric_visibility().unwrap();

    assert!(
        !wb.get_effective_field_visibility(&bob, &email_id).unwrap(),
        "an explicit Hidden toggle survives migration"
    );
    assert!(
        wb.get_effective_field_visibility(&bob, &phone_id).unwrap(),
        "the unruled sibling is still grandfathered to Visible"
    );
}

// @internal
#[test]
fn migration_is_idempotent_and_marker_gated() {
    let (wb, email_id, _phone_id, _bob) = pre_migration_world();

    assert!(wb.migrate_field_centric_visibility().unwrap());
    // Owner curates after migration: hide the email entry again.
    wb.set_own_field_private(&email_id).unwrap();

    let ran_again = wb.migrate_field_centric_visibility().unwrap();
    assert!(!ran_again, "second run is marker-gated to a no-op");
    let card = wb.own_card().unwrap().unwrap();
    assert!(
        !card.field_visibility().is_explicitly_everyone(&email_id),
        "the no-op run must not resurrect the materialized Everyone"
    );
}

// @internal
#[test]
fn migration_skips_group_assigned_fields_and_arms_repropagation() {
    let (wb, email_id, phone_id, bob) = pre_migration_world();
    // `Work` email is group-assigned; Bob stays outside the group.
    let group = wb.create_group("Team").unwrap();
    wb.set_group_field_visibility(group.id(), &email_id, true)
        .unwrap();
    // Repropagation owed from arrange noise is consumed before migrating.
    wb.initialize_sent_baseline(&bob).unwrap();
    wb.run_owed_repropagation().unwrap();
    let before = wb.storage().pending().count_all_pending_updates().unwrap();

    wb.migrate_field_centric_visibility().unwrap();

    let card = wb.own_card().unwrap().unwrap();
    assert!(
        !card.field_visibility().is_explicitly_everyone(&email_id),
        "a group-assigned entry gets no toggle materialization"
    );
    assert!(
        card.field_visibility().is_explicitly_everyone(&phone_id),
        "the unassigned entry is still grandfathered"
    );
    // Groups exist → the audience change must reach peers without waiting
    // for the next unrelated card edit.
    wb.run_owed_repropagation().unwrap();
    let after = wb.storage().pending().count_all_pending_updates().unwrap();
    assert!(
        after > before,
        "migration with groups present arms a repropagation pass"
    );
}

// @internal
#[test]
fn fresh_install_is_marked_at_identity_creation() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Owner").unwrap();
    let field = ContactField::new(FieldType::Email, "Work", "o@co.example", 0);
    let field_id = field.id().to_string();
    wb.add_own_field(field).unwrap();

    let ran = wb.migrate_field_centric_visibility().unwrap();
    assert!(!ran, "a fresh install never runs the sweep");
    let card = wb.own_card().unwrap().unwrap();
    assert!(
        !card.field_visibility().is_explicitly_everyone(&field_id),
        "fields added on a fresh install keep the hidden default"
    );
}

// @internal
#[test]
fn new_field_default_setting_materializes_visible_at_add_time() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Owner").unwrap();
    let mut flags = wb.load_settings_flags().unwrap();
    flags.new_field_default_visible = true;
    wb.save_settings_flags(&flags).unwrap();

    let field = ContactField::new(FieldType::Email, "Work", "o@co.example", 0);
    let field_id = field.id().to_string();
    wb.add_own_field(field).unwrap();

    let card = wb.own_card().unwrap().unwrap();
    assert!(
        card.field_visibility().is_explicitly_everyone(&field_id),
        "visible-default setting writes an explicit Everyone at add time"
    );
}

// @internal
#[test]
fn new_field_default_setting_off_leaves_field_unruled() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Owner").unwrap();

    let field = ContactField::new(FieldType::Email, "Work", "o@co.example", 0);
    let field_id = field.id().to_string();
    wb.add_own_field(field).unwrap();

    let card = wb.own_card().unwrap().unwrap();
    assert!(
        !card.field_visibility().contains(&field_id),
        "hidden default (setting off) writes no rule — the entry stays hidden"
    );
}

// @internal
#[test]
fn set_field_shown_arms_repropagation() {
    let (wb, email_id, _phone_id, bob) = pre_migration_world();
    wb.initialize_sent_baseline(&bob).unwrap();
    wb.run_owed_repropagation().unwrap();
    let before = wb.storage().pending().count_all_pending_updates().unwrap();

    wb.set_field_shown(&email_id, true).unwrap();
    wb.run_owed_repropagation().unwrap();

    let after = wb.storage().pending().count_all_pending_updates().unwrap();
    assert!(
        after > before,
        "the Visible/Hidden toggle must propagate to contacts"
    );
}
