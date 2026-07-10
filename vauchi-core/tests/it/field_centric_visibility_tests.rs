// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Truth table for the field-centric visibility model (2026-07-10 owner
//! decision, `2026-07-05-ungrouped-contacts-default-open`):
//! group-assigned fields are governed by group membership alone; unassigned
//! fields carry one Visible/Hidden toggle applying to every contact alike;
//! unruled defaults to hidden.

use vauchi_core::{Contact, ContactCard, SymmetricKey, Vauchi};

/// One `Vauchi` with two exchanged contacts: `alone` belongs to no group,
/// `member` belongs to group `g`. The own card carries field `f0`.
struct World {
    wb: Vauchi,
    alone: String,
    member: String,
    group: String,
}

fn world() -> World {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Owner").unwrap();
    let ids: Vec<String> = (0..2)
        .map(|i| {
            let contact = Contact::from_exchange(
                [(i as u8) + 1; 32],
                ContactCard::new(&format!("C{i}")),
                SymmetricKey::generate(),
                0,
            );
            let id = contact.id().to_string();
            wb.add_contact(contact).unwrap();
            id
        })
        .collect();
    let group = wb.create_group("G").unwrap().id().to_string();
    wb.add_contact_to_group(&group, &ids[1]).unwrap();
    World {
        wb,
        alone: ids[0].clone(),
        member: ids[1].clone(),
        group,
    }
}

fn sees(w: &World, contact: &str, field: &str) -> bool {
    w.wb.get_effective_field_visibility(contact, field).unwrap()
}

// @scenario: visibility_control :: New fields default to hidden for ungrouped contacts
#[test]
fn unruled_unassigned_field_is_hidden_from_every_contact() {
    let w = world();
    assert!(
        !sees(&w, &w.alone, "f0"),
        "no-group contact must not see an untoggled field"
    );
    assert!(
        !sees(&w, &w.member, "f0"),
        "group member must not see an untoggled field"
    );
}

// @scenario: visibility_control :: A Visible toggle reaches all contacts alike
#[test]
fn visible_toggle_on_unassigned_field_reaches_all_contacts() {
    let w = world();
    w.wb.set_own_field_public("f0").unwrap();
    assert!(
        sees(&w, &w.alone, "f0"),
        "Visible toggle must reach a no-group contact"
    );
    assert!(
        sees(&w, &w.member, "f0"),
        "Visible toggle must reach a group member too"
    );
}

// @scenario: visibility_control :: A Hidden toggle hides from all contacts alike
#[test]
fn hidden_toggle_on_unassigned_field_hides_from_all_contacts() {
    let w = world();
    w.wb.set_own_field_public("f0").unwrap();
    w.wb.set_own_field_private("f0").unwrap();
    assert!(
        !sees(&w, &w.alone, "f0"),
        "Hidden toggle must hide from a no-group contact"
    );
    assert!(
        !sees(&w, &w.member, "f0"),
        "Hidden toggle must hide from a group member"
    );
}

// @scenario: visibility_control :: Group-assigned fields follow group membership only
#[test]
fn group_assigned_field_visible_only_to_group_members() {
    let w = world();
    w.wb.set_group_field_visibility(&w.group, "f0", true)
        .unwrap();
    assert!(
        sees(&w, &w.member, "f0"),
        "group member must see the group-granted field"
    );
    assert!(
        !sees(&w, &w.alone, "f0"),
        "a contact outside every group must not see a group-assigned field"
    );
}

// @scenario: visibility_control :: Group assignment overrides a Visible toggle for non-members
#[test]
fn group_assignment_wins_over_visible_toggle_for_non_members() {
    let w = world();
    w.wb.set_own_field_public("f0").unwrap();
    w.wb.set_group_field_visibility(&w.group, "f0", true)
        .unwrap();
    assert!(
        !sees(&w, &w.alone, "f0"),
        "assigning a field to a group must close it to non-members even if toggled Visible"
    );
    assert!(
        sees(&w, &w.member, "f0"),
        "the group member keeps seeing it"
    );
}

// @scenario: visibility_control :: Per-contact overrides always win
#[test]
fn override_wins_in_both_partitions() {
    let w = world();
    // Deny an unassigned Visible field to one contact only.
    w.wb.set_own_field_public("f0").unwrap();
    w.wb.set_contact_visibility_override(&w.alone, "f0", false)
        .unwrap();
    assert!(
        !sees(&w, &w.alone, "f0"),
        "false override must hide a Visible field"
    );
    assert!(
        sees(&w, &w.member, "f0"),
        "override is per-contact, others unaffected"
    );
    // Grant a group-assigned field to a non-member.
    w.wb.set_group_field_visibility(&w.group, "f1", true)
        .unwrap();
    w.wb.set_contact_visibility_override(&w.alone, "f1", true)
        .unwrap();
    assert!(
        sees(&w, &w.alone, "f1"),
        "true override must open a group-assigned field to a non-member"
    );
}
