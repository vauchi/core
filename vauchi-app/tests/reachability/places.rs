// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `PlacesEngine` (ADR-051, Phase 4c).
//!
//! Like the Tags list: the only affordances are the per-row `request_delete`
//! (`ListItemAction`, not diffed) and the delete-confirm state (same
//! `screen_id`, deduped by the walker). Diffed `ActionPressed` set is empty.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{PlaceSummary, PlacesEngine, WorkflowEngine};

const HANDLED: &[&str] = &[];

fn factory() -> PlacesEngine {
    PlacesEngine::new(vec![
        PlaceSummary {
            id: "p1".into(),
            name: "Anchor Bar".into(),
        },
        PlaceSummary {
            id: "p2".into(),
            name: "Zurich HB".into(),
        },
    ])
}

// @internal
#[test]
fn places_screen_is_fully_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "places");
    assert_reachability_across_screens(factory, HANDLED);
}

// @internal
#[test]
fn places_screen_has_no_orphans() {
    let report = check_reachability(factory, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}
