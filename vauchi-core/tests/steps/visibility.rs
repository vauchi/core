// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reusable core step vocabulary for the contacts + cards + groups +
//! visibility domain (single-actor). Steps are parameterized and shared across
//! scenarios — the antidote to the free-form ~1.2-uses-per-pattern authoring
//! that left 97% of the suite unbound
//! (`problems/2026-07-04-cucumber-backgrounds-fail-silently`).
//!
//! Contacts and groups are addressed by name (resolved via `VauchiWorld`);
//! fields by their (unique) own-card label. Visibility assertions read the
//! *effective* visibility a contact would compute — the same verdict the peer
//! receives (`is_field_visible_by_label`).

use cucumber::{given, then, when};
use vauchi_core::{ContactField, FieldType};

use crate::VauchiWorld;

// ── Given: setup ────────────────────────────────────────────────

#[given(expr = "I have an existing identity as {string}")]
fn identity_as(world: &mut VauchiWorld, name: String) {
    world.vauchi.update_display_name(&name).unwrap();
}

#[given(expr = "I have a phone field {string} with value {string}")]
#[when(expr = "I add a phone field {string} with value {string}")]
fn phone_field(world: &mut VauchiWorld, label: String, value: String) {
    world
        .vauchi
        .add_own_field(ContactField::new(FieldType::Phone, &label, &value, 0))
        .unwrap();
}

#[given(expr = "I have an email field {string} with value {string}")]
fn email_field(world: &mut VauchiWorld, label: String, value: String) {
    world
        .vauchi
        .add_own_field(ContactField::new(FieldType::Email, &label, &value, 0))
        .unwrap();
}

#[given(expr = "I have an address field {string} with value {string}")]
fn address_field(world: &mut VauchiWorld, label: String, value: String) {
    world
        .vauchi
        .add_own_field(ContactField::new(FieldType::Address, &label, &value, 0))
        .unwrap();
}

#[given(expr = "I have a contact {string}")]
fn have_contact(world: &mut VauchiWorld, name: String) {
    world.add_test_contact(&name);
}

#[given(expr = "I have a visibility group {string}")]
#[given(expr = "I have a label {string}")]
fn have_group(world: &mut VauchiWorld, name: String) {
    let group = world.vauchi.create_group(&name).unwrap();
    world.groups.insert(name, group.id().to_string());
}

// ── When (and reusable Given): actions ──────────────────────────

#[given(expr = "contact {string} is in group {string}")]
#[given(expr = "contact {string} is in label {string}")]
#[when(expr = "I add contact {string} to group {string}")]
#[when(expr = "I add contact {string} to label {string}")]
fn add_to_group(world: &mut VauchiWorld, contact: String, group: String) {
    let cid = world.contact_id(&contact);
    let gid = world.group_id(&group);
    world.vauchi.add_contact_to_group(&gid, &cid).unwrap();
}

#[when(expr = "I remove contact {string} from group {string}")]
fn remove_from_group(world: &mut VauchiWorld, contact: String, group: String) {
    let cid = world.contact_id(&contact);
    let gid = world.group_id(&group);
    world.vauchi.remove_contact_from_group(&gid, &cid).unwrap();
}

#[when(expr = "I hide field {string} from contact {string}")]
fn hide_from(world: &mut VauchiWorld, field: String, contact: String) {
    let cid = world.contact_id(&contact);
    let fid = world.own_field_id(&field);
    world
        .vauchi
        .set_field_private_and_repropagate(&cid, &fid)
        .unwrap();
}

#[when(expr = "I make field {string} visible to contact {string}")]
fn show_to(world: &mut VauchiWorld, field: String, contact: String) {
    let cid = world.contact_id(&contact);
    let fid = world.own_field_id(&field);
    world
        .vauchi
        .set_field_public_and_repropagate(&cid, &fid)
        .unwrap();
}

/// Restrict a field to a comma-separated set of contacts: every known contact
/// in the set gets a visible override, everyone else a hidden override.
#[when(expr = "I make field {string} visible only to contacts {string}")]
fn restrict_to(world: &mut VauchiWorld, field: String, allowed_csv: String) {
    let fid = world.own_field_id(&field);
    let allowed: Vec<String> = allowed_csv
        .split(',')
        .map(|s| world.contact_id(s.trim()))
        .collect();
    for cid in world.contacts.values().cloned().collect::<Vec<_>>() {
        if allowed.contains(&cid) {
            world
                .vauchi
                .set_field_public_and_repropagate(&cid, &fid)
                .unwrap();
        } else {
            world
                .vauchi
                .set_field_private_and_repropagate(&cid, &fid)
                .unwrap();
        }
    }
}

#[when(expr = "I make field {string} private")]
fn make_private(world: &mut VauchiWorld, field: String) {
    let fid = world.own_field_id(&field);
    world.vauchi.set_own_field_private(&fid).unwrap();
}

/// A field visible *only* to a group = removed from the public base (so
/// ungrouped contacts don't see it) AND granted to the group (so its members
/// do). Grouped non-members stay default-closed (ADR-054 D3).
#[given(expr = "I make field {string} visible only to group {string}")]
#[given(expr = "I make field {string} visible only to label {string}")]
#[when(expr = "I make field {string} visible only to group {string}")]
#[when(expr = "I make field {string} visible only to label {string}")]
fn visible_only_to_group(world: &mut VauchiWorld, field: String, group: String) {
    let fid = world.own_field_id(&field);
    let gid = world.group_id(&group);
    world.vauchi.set_own_field_private(&fid).unwrap();
    world
        .vauchi
        .set_group_field_visibility(&gid, &fid, true)
        .unwrap();
}

// ── Then: assertions ────────────────────────────────────────────

#[then(expr = "contact {string} can see my {string} field")]
fn can_see(world: &mut VauchiWorld, contact: String, field: String) {
    let cid = world.contact_id(&contact);
    assert!(
        world
            .vauchi
            .is_field_visible_by_label(&cid, &field)
            .unwrap(),
        "expected {contact} to see field {field}"
    );
}

#[then(expr = "contact {string} cannot see my {string} field")]
fn cannot_see(world: &mut VauchiWorld, contact: String, field: String) {
    let cid = world.contact_id(&contact);
    assert!(
        !world
            .vauchi
            .is_field_visible_by_label(&cid, &field)
            .unwrap(),
        "expected {contact} NOT to see field {field}"
    );
}

#[then(expr = "all contacts can see my {string} field")]
fn all_can_see(world: &mut VauchiWorld, field: String) {
    for (name, cid) in &world.contacts {
        assert!(
            world.vauchi.is_field_visible_by_label(cid, &field).unwrap(),
            "expected {name} to see field {field}"
        );
    }
}

#[then(expr = "group {string} contains contact {string}")]
fn group_contains(world: &mut VauchiWorld, group: String, contact: String) {
    let gid = world.group_id(&group);
    let cid = world.contact_id(&contact);
    let members = world.vauchi.get_group_members(&gid).unwrap();
    assert!(
        members.iter().any(|c| c.id() == cid),
        "expected group {group} to contain {contact}"
    );
}

#[then(expr = "group {string} does not contain contact {string}")]
fn group_not_contains(world: &mut VauchiWorld, group: String, contact: String) {
    let gid = world.group_id(&group);
    let cid = world.contact_id(&contact);
    let members = world.vauchi.get_group_members(&gid).unwrap();
    assert!(
        !members.iter().any(|c| c.id() == cid),
        "expected group {group} NOT to contain {contact}"
    );
}
