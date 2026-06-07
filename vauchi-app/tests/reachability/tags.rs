// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `TagsEngine` (ADR-051 contact annotations,
//! Phase 4b — tag management list).
//!
//! The Tags list has no `ScreenAction`s — its only affordances are the
//! per-row `request_delete` (`UserAction::ListItemAction`, which the
//! walker emits but `check_reachability` does not diff) and the
//! delete-confirm state, which keeps `screen_id == "tags"` so the BFS
//! walker dedupes it (same pattern as the contact-detail delete confirm).
//! So the diffed `ActionPressed` affordance set is empty.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{TagSummary, TagsEngine, WorkflowEngine};

const HANDLED: &[&str] = &[];

fn factory() -> TagsEngine {
    TagsEngine::new(vec![
        TagSummary {
            id: "t1".into(),
            name: "climbing".into(),
            member_count: 3,
        },
        TagSummary {
            id: "t2".into(),
            name: "work".into(),
            member_count: 1,
        },
    ])
}

// @internal
#[test]
fn tags_screen_is_fully_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "tags");
    assert_reachability_across_screens(factory, HANDLED);
}

// @internal
#[test]
fn tags_screen_has_no_orphans() {
    let report = check_reachability(factory, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}
