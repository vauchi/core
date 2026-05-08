// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `ContactListEngine`.
//!
//! Closes the CC-22 gap surfaced in the 2026-05-08 architecture/ADR
//! compliance audit (finding P-11): the engine is on every install's
//! main screen, gained `archive`/`hide`/`delete` row actions in core
//! 4abf1a99 (`ContactActionKind` mapping), and was the only sizeable
//! engine without a reachability file.
//!
//! `contact_list` reuses the same `screen_id` after a group filter is
//! applied (the engine flips `group_filter` and re-renders). The BFS
//! walker dedupes by `screen_id` so the post-filter affordances
//! (`filter_group_clear`) are not revisited from the initial state —
//! HANDLED is sized to the initial screen's emissions. Adding a
//! second factory variant for the post-filter screen is plausible
//! follow-up work but would not catch the bug class this gap was
//! about (a row-action arm landing without an affordance).
//!
//! Per-row actions (`UserAction::ListItemAction { action_id }`) are
//! emitted by the walker since this audit (commit e28be281) but are
//! not yet diffed by `check_reachability` — that filter is
//! `ActionPressed`-only. The audit's notes flag this as a follow-up.

use std::collections::HashMap;

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{ContactListEngine, IndexedItem, Item, WorkflowEngine};

/// `ScreenAction` ids emitted on the initial `contact_list` screen
/// when the engine is constructed with one group and one contact:
///
/// - `add_contact` — always present (intercepted by `AppEngine` for
///   navigation; falls through to a no-op screen refresh in standalone).
/// - `filter_group:work` — one chip per available group (`Work` here).
/// - `view_archived` — always present.
/// - `find_duplicates` — always present.
///
/// Not exercised by this fixture (logged in module doc-comment):
/// `filter_group_clear` (post-filter state, deduped by `screen_id`),
/// `go_exchange` (empty-state only).
const HANDLED: &[&str] = &[
    "add_contact",
    "filter_group:work",
    "view_archived",
    "find_duplicates",
];

fn factory() -> ContactListEngine {
    let item = Item {
        id: "c-alice".into(),
        name: "Alice".into(),
        subtitle: None,
        avatar_initials: "A".into(),
        status: None,
        actions: vec![],
        a11y: None,
    };
    let indexed: IndexedItem = item.into();
    let groups = vec![("work".to_string(), "Work".to_string())];
    let mut memberships: HashMap<String, Vec<String>> = HashMap::new();
    memberships.insert("work".into(), vec!["c-alice".into()]);
    ContactListEngine::with_groups(vec![indexed], groups, memberships)
}

// @internal
#[test]
fn contact_list_screen_is_fully_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "contact_list");
    assert_reachability_across_screens(factory, HANDLED);
}

// @internal
#[test]
fn contact_list_has_no_orphans() {
    let report = check_reachability(factory, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}
