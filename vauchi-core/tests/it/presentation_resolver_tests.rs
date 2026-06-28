// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `GroupManager::resolve_presentation` (ADR-054 Phase 3, Option B).
//!
//! Presentation (name/bio/avatar — how the user is known to a recipient, not
//! the cryptographic identity) is drawn from a single winning group — smallest
//! by membership, ties broken by `(created_at, id)` — falling back to the
//! default card field-by-field, or to the default entirely when ungrouped.

use proptest::prelude::*;
use std::collections::HashSet;
use vauchi_core::contact::{Group, GroupManager, ResolvedPresentation};

fn default_presentation() -> ResolvedPresentation {
    ResolvedPresentation {
        display_name: "Mattia Egloff".to_string(),
        bio: Some("default bio".to_string()),
        avatar: Some(b"default-avatar".to_vec()),
    }
}

/// Builds a group directly (bypassing validation) for precise control over
/// id / created_at / members / overrides.
fn group(
    id: &str,
    created_at: u64,
    members: &[&str],
    name_override: Option<&str>,
    bio_override: Option<&str>,
    avatar_override: Option<&[u8]>,
) -> Group {
    Group::from_storage(
        id.to_string(),
        format!("name-{id}"),
        members.iter().map(|m| m.to_string()).collect(),
        HashSet::new(),
        name_override.map(String::from),
        bio_override.map(String::from),
        avatar_override.map(<[u8]>::to_vec),
        created_at,
        created_at,
    )
}

// @internal
#[test]
fn ungrouped_contact_resolves_to_default() {
    let mgr = GroupManager::new();
    let default = default_presentation();

    let resolved = mgr.resolve_presentation("alice", &default);

    assert_eq!(resolved, default);
}

// @internal
#[test]
fn single_group_overrides_win() {
    let mut mgr = GroupManager::new();
    mgr.insert_loaded_group(group(
        "g1",
        100,
        &["alice"],
        Some("Mom"),
        Some("loves gardening"),
        Some(b"group-avatar"),
    ));

    let resolved = mgr.resolve_presentation("alice", &default_presentation());

    assert_eq!(resolved.display_name, "Mom");
    assert_eq!(resolved.bio.as_deref(), Some("loves gardening"));
    assert_eq!(resolved.avatar.as_deref(), Some(b"group-avatar".as_slice()));
}

// @internal
#[test]
fn overrides_fall_back_field_by_field() {
    let mut mgr = GroupManager::new();
    // Winner has only a bio override; name and avatar must fall back.
    mgr.insert_loaded_group(group("g1", 100, &["alice"], None, Some("only bio"), None));

    let default = default_presentation();
    let resolved = mgr.resolve_presentation("alice", &default);

    assert_eq!(resolved.display_name, default.display_name);
    assert_eq!(resolved.bio.as_deref(), Some("only bio"));
    assert_eq!(resolved.avatar, default.avatar);
}

// @internal
#[test]
fn smallest_group_wins() {
    let mut mgr = GroupManager::new();
    mgr.insert_loaded_group(group(
        "big",
        100,
        &["alice", "bob", "carol"],
        Some("Big"),
        None,
        None,
    ));
    mgr.insert_loaded_group(group("small", 100, &["alice"], Some("Small"), None, None));

    let resolved = mgr.resolve_presentation("alice", &default_presentation());

    assert_eq!(resolved.display_name, "Small");
}

// @internal
#[test]
fn size_tie_broken_by_older_created_at() {
    let mut mgr = GroupManager::new();
    mgr.insert_loaded_group(group("newer", 200, &["alice"], Some("Newer"), None, None));
    mgr.insert_loaded_group(group("older", 100, &["alice"], Some("Older"), None, None));

    let resolved = mgr.resolve_presentation("alice", &default_presentation());

    assert_eq!(resolved.display_name, "Older");
}

// @internal
#[test]
fn size_and_created_at_tie_broken_by_id() {
    let mut mgr = GroupManager::new();
    // Same size, same created_at: lexicographically smaller id wins.
    mgr.insert_loaded_group(group("bbb", 100, &["alice"], Some("Bee"), None, None));
    mgr.insert_loaded_group(group("aaa", 100, &["alice"], Some("Aay"), None, None));

    let resolved = mgr.resolve_presentation("alice", &default_presentation());

    assert_eq!(resolved.display_name, "Aay");
}

/// ADR-032: a contact whose winning group carries no overrides resolves to a
/// `ResolvedPresentation` byte-identical to the default card — indistinguishable
/// from an ungrouped contact / a duress decoy.
// @internal
#[test]
fn empty_overrides_resolve_indistinguishably_from_default() {
    let mut mgr = GroupManager::new();
    mgr.insert_loaded_group(group("g1", 100, &["alice"], None, None, None));

    let default = default_presentation();
    let grouped = mgr.resolve_presentation("alice", &default);
    let ungrouped = mgr.resolve_presentation("stranger", &default);

    assert_eq!(grouped, default);
    assert_eq!(grouped, ungrouped);
}

proptest! {
    /// CC-13: the winner is the deterministic minimum by `(count, created_at,
    /// id)` regardless of HashMap insertion/iteration order.
    // @internal
    #[test]
    fn winner_is_deterministic_min(
        specs in proptest::collection::vec((0u64..4, 0usize..4), 1..8),
    ) {
        let default = default_presentation();
        let mut mgr = GroupManager::new();

        for (i, (created_at, extra)) in specs.iter().enumerate() {
            let id = format!("g{i:02}");
            let mut contacts: HashSet<String> = HashSet::new();
            contacts.insert("alice".to_string());
            for j in 0..*extra {
                contacts.insert(format!("m{i}_{j}"));
            }
            mgr.insert_loaded_group(Group::from_storage(
                id.clone(),
                format!("name-{id}"),
                contacts,
                HashSet::new(),
                Some(id.clone()), // name override == id, so we can identify the winner
                None,
                None,
                *created_at,
                *created_at,
            ));
        }

        // Independent expected winner: min by (count, created_at, id).
        let expected = specs
            .iter()
            .enumerate()
            .map(|(i, (ct, extra))| (1 + *extra, *ct, format!("g{i:02}")))
            .min_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)))
            .map(|(_, _, id)| id)
            .unwrap();

        let resolved = mgr.resolve_presentation("alice", &default);
        prop_assert_eq!(resolved.display_name, expected);
    }
}
