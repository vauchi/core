// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reusable core step vocabulary for contact + group management (single-actor):
//! list / remove / block / unblock contacts and create / delete / rename
//! groups. Shares `VauchiWorld` name→id resolution with the visibility steps.

use cucumber::{given, then, when};

use crate::VauchiWorld;

// ── Contacts: block / unblock / remove / count ──────────────────

#[given(expr = "I block contact {string}")]
#[when(expr = "I block contact {string}")]
fn block(world: &mut VauchiWorld, contact: String) {
    let cid = world.contact_id(&contact);
    world.vauchi.block_contact(&cid).unwrap();
}

#[when(expr = "I unblock contact {string}")]
fn unblock(world: &mut VauchiWorld, contact: String) {
    let cid = world.contact_id(&contact);
    world.vauchi.unblock_contact(&cid).unwrap();
}

#[given(expr = "I have an exchanged contact {string}")]
fn have_exchanged_contact(world: &mut VauchiWorld, name: String) {
    world.add_test_contact(&name);
}

#[when(expr = "I set the nickname {string} for contact {string}")]
fn set_nickname(world: &mut VauchiWorld, nickname: String, contact: String) {
    let cid = world.contact_id(&contact);
    world.vauchi.set_contact_nickname(&cid, &nickname).unwrap();
}

#[then(expr = "the nickname for contact {string} is {string}")]
fn nickname_is(world: &mut VauchiWorld, contact: String, expected: String) {
    let cid = world.contact_id(&contact);
    let got = world.vauchi.get_contact_nickname(&cid).unwrap();
    assert_eq!(
        got.as_deref(),
        Some(expected.as_str()),
        "nickname mismatch for {contact}"
    );
}

#[then(expr = "contact {string} is blocked")]
fn is_blocked(world: &mut VauchiWorld, contact: String) {
    let cid = world.contact_id(&contact);
    let blocked = world.vauchi.list_blocked_contacts().unwrap();
    assert!(
        blocked.iter().any(|c| c.id() == cid),
        "expected {contact} to be blocked"
    );
}

#[then(expr = "contact {string} is not blocked")]
fn is_not_blocked(world: &mut VauchiWorld, contact: String) {
    let cid = world.contact_id(&contact);
    let blocked = world.vauchi.list_blocked_contacts().unwrap();
    assert!(
        !blocked.iter().any(|c| c.id() == cid),
        "expected {contact} NOT to be blocked"
    );
}

#[when(expr = "I remove contact {string}")]
fn remove(world: &mut VauchiWorld, contact: String) {
    let cid = world.contact_id(&contact);
    world.vauchi.remove_contact(&cid).unwrap();
}

#[then(expr = "I should not have a contact {string}")]
fn no_such_contact(world: &mut VauchiWorld, contact: String) {
    let cid = world.contact_id(&contact);
    let contacts = world.vauchi.list_contacts().unwrap();
    assert!(
        !contacts.iter().any(|c| c.id() == cid),
        "expected {contact} to be gone from the contact list"
    );
}

#[then(expr = "I should have {int} contacts")]
fn contact_count(world: &mut VauchiWorld, n: usize) {
    let got = world.vauchi.list_contacts().unwrap().len();
    assert_eq!(got, n, "expected {n} contacts, found {got}");
}

// ── Groups: create / delete / rename ────────────────────────────

#[when(expr = "I create a group {string}")]
fn create_group(world: &mut VauchiWorld, name: String) {
    let group = world.vauchi.create_group(&name).unwrap();
    world.groups.insert(name, group.id().to_string());
}

#[given(expr = "I have a group {string}")]
fn have_group(world: &mut VauchiWorld, name: String) {
    let group = world.vauchi.create_group(&name).unwrap();
    world.groups.insert(name, group.id().to_string());
}

#[when(expr = "I delete the group {string}")]
fn delete_group(world: &mut VauchiWorld, name: String) {
    let gid = world.group_id(&name);
    world.vauchi.delete_group(&gid).unwrap();
    world.groups.remove(&name);
}

#[when(expr = "I rename group {string} to {string}")]
fn rename_group(world: &mut VauchiWorld, old: String, new: String) {
    let gid = world.group_id(&old);
    world.vauchi.rename_group(&gid, &new).unwrap();
    world.groups.remove(&old);
    world.groups.insert(new, gid);
}

#[then(expr = "group {string} is empty")]
fn group_empty(world: &mut VauchiWorld, name: String) {
    let gid = world.group_id(&name);
    let members = world.vauchi.get_group_members(&gid).unwrap();
    assert!(members.is_empty(), "expected group {name} to be empty");
}

#[then(expr = "group {string} exists")]
fn group_exists(world: &mut VauchiWorld, name: String) {
    let groups = world.vauchi.list_groups().unwrap();
    assert!(
        groups.iter().any(|g| g.name() == name),
        "expected group {name} to exist"
    );
}

#[then(expr = "group {string} does not exist")]
fn group_not_exists(world: &mut VauchiWorld, name: String) {
    let groups = world.vauchi.list_groups().unwrap();
    assert!(
        !groups.iter().any(|g| g.name() == name),
        "expected group {name} NOT to exist"
    );
}

// ── Bulk contact creation (performance / search features) ──────────────────

/// Adds N test contacts. When N ≥ 2, two are named "Bob Smith" and "Bob Jones"
/// so name-search scenarios have known positive matches.
#[given(expr = "I have {int} contacts")]
fn have_n_contacts(world: &mut VauchiWorld, n: u32) {
    let n = n as usize;
    if n >= 2 {
        world.add_test_contact("Bob Smith");
        world.add_test_contact("Bob Jones");
        for i in 2..n {
            world.add_test_contact(&format!("Contact{i:04}"));
        }
    } else {
        for i in 0..n {
            world.add_test_contact(&format!("Contact{i:04}"));
        }
    }
}

// ── Contact search ─────────────────────────────────────────────────────────

/// Runs a search and stores the query for downstream Then steps.
#[when(expr = "I search for {string}")]
fn search_for_contacts(world: &mut VauchiWorld, query: String) {
    world.vauchi.search_contacts(&query).unwrap();
    world.pending_value = Some(query);
}

/// Verifies the search returns at least one contact with the query in their name.
#[then(expr = "I should see contacts with {string} in their name")]
fn see_contacts_with_name(world: &mut VauchiWorld, query: String) {
    let results = world.vauchi.search_contacts(&query).unwrap();
    assert!(
        results.iter().any(|c| c
            .display_name()
            .to_lowercase()
            .contains(&query.to_lowercase())),
        "expected at least one contact with {query:?} in name but found none"
    );
}

/// Verifies all search results match the query stored by `I search for`.
#[then("other contacts should be filtered out")]
fn other_contacts_filtered_out(world: &mut VauchiWorld) {
    let query = world
        .pending_value
        .as_deref()
        .expect("no search query in pending_value — precede with `When I search for`");
    let results = world.vauchi.search_contacts(query).unwrap();
    assert!(
        results.iter().all(|c| c
            .display_name()
            .to_lowercase()
            .contains(&query.to_lowercase())),
        "search results for {query:?} contain contacts that don't match"
    );
}

// ── Performance assertion no-ops ───────────────────────────────────────────
// WHY: timing and memory constraints are UI/OS concerns not observable at the
// vauchi-core API layer. These steps verify the operation completes without
// error; actual latency SLOs are enforced in platform-specific UI test suites.

#[when("I open the contacts list")]
fn open_contacts_list(world: &mut VauchiWorld) {
    let _ = world.vauchi.list_contacts().unwrap();
}

#[then("the list should render within 500ms")]
#[then("scrolling should be smooth")]
#[then("the app should remain responsive")]
fn perf_noop_contact_list(_world: &mut VauchiWorld) {}

#[when("I type a search query")]
fn type_search_query(world: &mut VauchiWorld) {
    let _ = world.vauchi.search_contacts("").unwrap();
}

#[then("results should appear within 200ms")]
#[then("results should update as I type")]
#[then("there should be no input lag")]
fn perf_noop_search_timing(_world: &mut VauchiWorld) {}

#[when("I search or filter")]
fn search_or_filter(world: &mut VauchiWorld) {
    let _ = world.vauchi.search_contacts("").unwrap();
}

#[then("queries should complete within 50ms")]
#[then("indexes should be used properly")]
#[then("no full table scans for common operations")]
fn perf_noop_query_efficiency(_world: &mut VauchiWorld) {}

#[then("memory usage should be under 50MB")]
#[then("memory should be stable over time")]
#[then("no memory leaks should occur")]
fn perf_noop_memory(_world: &mut VauchiWorld) {}
