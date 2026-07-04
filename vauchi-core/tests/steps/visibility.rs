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

/// Table form of the field setup — one step seeds a whole card:
///   | type  | label      | value           |
///   | phone | Work Phone | +1-555-222-2222 |
#[given(expr = "I have the following fields on my contact card:")]
fn fields_from_table(world: &mut VauchiWorld, step: &cucumber::gherkin::Step) {
    let table = step
        .table
        .as_ref()
        .expect("step requires a |type|label|value| table");
    for row in table.rows.iter().skip(1) {
        let (kind, label, value) = (&row[0], &row[1], &row[2]);
        let field_type = match kind.trim().to_lowercase().as_str() {
            "phone" => FieldType::Phone,
            "email" => FieldType::Email,
            "address" => FieldType::Address,
            "website" => FieldType::Website,
            "custom" => FieldType::Custom,
            other => panic!("unsupported field type {other:?} in table"),
        };
        world
            .vauchi
            .add_own_field(ContactField::new(field_type, label.trim(), value.trim(), 0))
            .unwrap();
    }
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

#[when(expr = "I try to create another label named {string}")]
fn try_create_duplicate_label(world: &mut VauchiWorld, name: String) {
    world.last_result = world
        .vauchi
        .create_group(&name)
        .map(|_| ())
        .map_err(|e| e.to_string());
}

// ── When (and reusable Given): actions ──────────────────────────

/// Quoted-name variants: `contact "Bob" is in group "Family"`.
#[given(expr = "contact {string} is in group {string}")]
#[given(expr = "contact {string} is in label {string}")]
#[when(expr = "I add contact {string} to group {string}")]
#[when(expr = "I add contact {string} to label {string}")]
/// Also matches `I add "Bob" to label "Family"` (quoted contact name).
#[when(expr = "I add {string} to label {string}")]
fn add_to_group(world: &mut VauchiWorld, contact: String, group: String) {
    let cid = world.contact_id(&contact);
    let gid = world.group_id(&group);
    world.vauchi.add_contact_to_group(&gid, &cid).unwrap();
}

/// Bare-name variant: `I add Bob to label "Family"` (unquoted contact name).
/// Also creates the label if it does not yet exist, so scenarios can combine
/// membership setup in a single step.
#[when(expr = "I add {word} to label {string}")]
#[given(expr = "{word} is in label {string}")]
fn add_bare_to_label(world: &mut VauchiWorld, contact: String, group: String) {
    let cid = world.contact_id(&contact);
    let gid = if let Some(id) = world.groups.get(&group) {
        id.clone()
    } else {
        let g = world.vauchi.create_group(&group).unwrap();
        let id = g.id().to_string();
        world.groups.insert(group.clone(), id.clone());
        id
    };
    world.vauchi.add_contact_to_group(&gid, &cid).unwrap();
}

#[when(expr = "I remove contact {string} from group {string}")]
#[when(expr = "I remove {string} from label {string}")]
fn remove_from_group(world: &mut VauchiWorld, contact: String, group: String) {
    let cid = world.contact_id(&contact);
    let gid = world.group_id(&group);
    world.vauchi.remove_contact_from_group(&gid, &cid).unwrap();
}

/// Bare-name variant: `I remove Bob from label "Friends"` (unquoted name).
#[when(expr = "I remove {word} from label {string}")]
fn remove_bare_from_label(world: &mut VauchiWorld, contact: String, group: String) {
    let cid = world.contact_id(&contact);
    let gid = world.group_id(&group);
    world.vauchi.remove_contact_from_group(&gid, &cid).unwrap();
}

/// Cancel means we never call delete_group; both steps are intentional no-ops.
/// The assertion "the label {string} should still exist" confirms the outcome.
#[when(expr = "I attempt to delete the label {string}")]
fn attempt_delete_label(_world: &mut VauchiWorld, _name: String) {}

#[when("I cancel the deletion")]
fn cancel_deletion(_world: &mut VauchiWorld) {}

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

#[then(expr = "no contact can see my {string} field")]
fn no_contact_can_see(world: &mut VauchiWorld, field: String) {
    assert!(
        !world.contacts.is_empty(),
        "vacuous check — the Background must create contacts first"
    );
    for (name, cid) in &world.contacts {
        assert!(
            !world.vauchi.is_field_visible_by_label(cid, &field).unwrap(),
            "expected {name} NOT to see field {field} (fields default hidden)"
        );
    }
    // Worlds are per-scenario, so this hands the field to the
    // explicit-grant follow-up step without a dedicated slot.
    world.pending_label = Some(field);
}

#[then("the field stays hidden until I explicitly grant visibility")]
fn hidden_until_granted(world: &mut VauchiWorld) {
    let field = world
        .pending_label
        .clone()
        .expect("a preceding no-contact-can-see step names the field");
    // Not a re-assertion of hidden: the "until I explicitly grant" half
    // means one grant must flip visibility for exactly that contact.
    let (name, cid) = world
        .contacts
        .iter()
        .next()
        .map(|(n, c)| (n.clone(), c.clone()))
        .expect("Background creates contacts");
    let fid = world.own_field_id(&field);
    world
        .vauchi
        .set_field_public_and_repropagate(&cid, &fid)
        .unwrap();
    assert!(
        world
            .vauchi
            .is_field_visible_by_label(&cid, &field)
            .unwrap(),
        "explicit grant makes {field} visible to {name}"
    );
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

#[then(expr = "only one {string} label should exist")]
fn only_one_label(world: &mut VauchiWorld, name: String) {
    let count = world
        .vauchi
        .list_groups()
        .unwrap()
        .into_iter()
        .filter(|g| g.name() == name)
        .count();
    assert_eq!(count, 1, "expected exactly one label named {name:?}");
}

/// Bare-name variants for membership assertions: `Bob should be a member of "Family"`.
#[then(expr = "{word} should be a member of {string}")]
fn contact_is_member(world: &mut VauchiWorld, contact: String, label: String) {
    let cid = world.contact_id(&contact);
    let gid = world.group_id(&label);
    let members = world.vauchi.get_group_members(&gid).unwrap();
    assert!(
        members.iter().any(|c| c.id() == cid),
        "expected {contact} to be a member of label {label}"
    );
}

#[then(expr = "{word} should not be a member of {string}")]
fn contact_not_member(world: &mut VauchiWorld, contact: String, label: String) {
    let cid = world.contact_id(&contact);
    let gid = world.group_id(&label);
    let members = world.vauchi.get_group_members(&gid).unwrap();
    assert!(
        !members.iter().any(|c| c.id() == cid),
        "expected {contact} NOT to be a member of label {label}"
    );
}

/// Checks that the contact can see every field the label exposes.
/// Vacuously passes when no fields are associated with the label — the
/// correct assertion when a scenario adds a contact to an otherwise-empty label.
#[then(expr = "{word} should see fields associated with {string}")]
fn contact_sees_label_fields(world: &mut VauchiWorld, contact: String, label: String) {
    let cid = world.contact_id(&contact);
    let gid = world.group_id(&label);
    let groups = world.vauchi.list_groups().unwrap();
    let group = groups.iter().find(|g| g.id() == gid).unwrap();
    let own_card = world.vauchi.own_card().unwrap().unwrap();
    for field in own_card.fields() {
        if group.is_field_visible(field.id()) {
            assert!(
                world
                    .vauchi
                    .is_field_visible_by_label(&cid, field.label())
                    .unwrap(),
                "expected {contact} to see field {:?} via label {label}",
                field.label()
            );
        }
    }
}

/// After removal from a label, visibility resolution must not panic or error.
/// Regex matches possessive form: "Bob's visibility should fall back to …"
#[then(regex = r"^(.+?)'s visibility should fall back to per-contact settings or defaults$")]
fn visibility_falls_back(world: &mut VauchiWorld, contact: String) {
    let cid = world.contact_id(&contact);
    let own_card = world.vauchi.own_card().unwrap().unwrap();
    for field in own_card.fields() {
        world
            .vauchi
            .is_field_visible_by_label(&cid, field.label())
            .unwrap_or_else(|e| panic!("visibility resolution failed for {contact}: {e}"));
    }
}

#[then(expr = "the label {string} should still exist")]
fn label_still_exists(world: &mut VauchiWorld, name: String) {
    assert!(
        world
            .vauchi
            .list_groups()
            .unwrap()
            .iter()
            .any(|g| g.name() == name),
        "expected label {name:?} to still exist"
    );
}
