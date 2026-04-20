// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Static reachability check between a `WorkflowEngine`'s declared
//! consumer set and the affordance set emitted by `ScreenWalker`.
//!
//! # Bug class caught
//!
//! - **Orphan handlers** — `handle_action` arms keyed on some
//!   `action_id`, but no rendered `ScreenAction` emits that id.
//!   The handler is unreachable from the UI and the fact that it
//!   works in isolation (e.g. driven by a proptest with hand-listed
//!   ids) masks the defect. Today's smoking gun:
//!   `core/vauchi-app/src/ui/onboarding.rs:606` handles
//!   `"submit_custom_group"` but no `ScreenAction` with that id is
//!   ever emitted by `onboarding.rs`.
//! - **Orphan affordances** — a rendered `ScreenAction` or other
//!   affordance emits some id, but no handler arm consumes it. The
//!   user taps and nothing happens (or hits a silent fallback arm).
//!
//! # What this file does NOT do yet
//!
//! Phase 1 Task 1.1 MVP: the diff is against `engine.current_screen()`
//! only. A follow-up unions `walk_actions` across every `ScreenModel`
//! reachable by driving `handle_action` — that upgrade lives in
//! `screen_walker::all_reachable_screens` (currently a stub).
//!
//! Dynamic per-arm tracing (plan Task 1.2) is a separate mechanism
//! — it detects handlers whose arm *fires* but silently no-ops,
//! which the static diff cannot see.

use std::collections::BTreeSet;

use super::screen_walker::walk_actions;
use crate::ui::{UserAction, WorkflowEngine};

/// Outcome of a static reachability check.
///
/// Stored as sorted sets so assertion messages are deterministic
/// across proptest shrinks and cross-platform reproductions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReachabilityReport {
    /// Action ids declared by the engine that no affordance emits.
    pub orphan_handlers: BTreeSet<String>,
    /// Action ids emitted by affordances that the engine does not
    /// declare as a handler consumer.
    pub orphan_affordances: BTreeSet<String>,
}

impl ReachabilityReport {
    /// `true` when both sides are empty — every declared handler
    /// is reachable from some affordance and every affordance hits
    /// a declared handler.
    pub fn is_reachable(&self) -> bool {
        self.orphan_handlers.is_empty() && self.orphan_affordances.is_empty()
    }
}

/// Diff declared handler `action_id`s against the action ids that
/// `walk_actions(engine.current_screen())` emits as
/// `UserAction::ActionPressed`.
///
/// Pass-through ids — affordances that map to a non-`ActionPressed`
/// user action (`TextChanged`, `ItemToggled`, `ListItemSelected`)
/// — are not included in either side; those shapes are validated
/// by the dynamic harness added in Task 1.2.
pub fn check_static_reachability<E: WorkflowEngine + ?Sized>(
    engine: &E,
    declared_action_ids: &[&str],
) -> ReachabilityReport {
    let walked = walk_actions(&engine.current_screen());
    let walked_action_ids: BTreeSet<String> = walked
        .iter()
        .filter_map(|action| match action {
            UserAction::ActionPressed { action_id } => Some(action_id.clone()),
            _ => None,
        })
        .collect();

    let declared: BTreeSet<String> = declared_action_ids
        .iter()
        .map(|id| (*id).to_string())
        .collect();

    ReachabilityReport {
        orphan_handlers: declared.difference(&walked_action_ids).cloned().collect(),
        orphan_affordances: walked_action_ids.difference(&declared).cloned().collect(),
    }
}

/// Panicking assertion form of [`check_static_reachability`].
///
/// Intended for use in `#[test]` or `proptest!` blocks.
#[track_caller]
pub fn assert_reachability<E: WorkflowEngine + ?Sized>(engine: &E, declared_action_ids: &[&str]) {
    let report = check_static_reachability(engine, declared_action_ids);
    assert!(
        report.is_reachable(),
        "ScreenModel reachability violation on screen `{screen_id}`:\n\
         \torphan handlers (declared, no affordance emits): {orphan_handlers:?}\n\
         \torphan affordances (emitted, no declared handler): {orphan_affordances:?}",
        screen_id = engine.current_screen().screen_id,
        orphan_handlers = report.orphan_handlers,
        orphan_affordances = report.orphan_affordances,
    );
}

// INLINE_TEST_REQUIRED: the harness's correctness is defined by the
// pair of toy-engine regressions — one with an orphan handler, one
// with an orphan affordance. Splitting them into a separate file
// would break the single-diff proof that the harness catches both
// classes (plan Phase 5 Task 5.1 meta-test).
#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification_types::PendingNotification;
    use crate::ui::action::ActionResult;
    use crate::ui::screen::{ActionStyle, ScreenAction, ScreenModel};

    /// Minimal `WorkflowEngine` that returns a fixed `ScreenModel`
    /// and does nothing on `handle_action`. Enough to exercise
    /// the walker-vs-declared diff; real engines plug in later.
    struct ToyEngine {
        screen: ScreenModel,
    }

    impl WorkflowEngine for ToyEngine {
        fn current_screen(&self) -> ScreenModel {
            self.screen.clone()
        }

        fn handle_action(&mut self, _action: UserAction) -> ActionResult {
            ActionResult::UpdateScreen(self.screen.clone())
        }

        fn poll_notifications(&mut self) -> Vec<PendingNotification> {
            Vec::new()
        }
    }

    fn screen_with_actions(ids: &[&str]) -> ScreenModel {
        ScreenModel::new(
            "toy",
            "Toy",
            Vec::new(),
            ids.iter()
                .map(|id| ScreenAction {
                    id: (*id).into(),
                    label: (*id).into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                })
                .collect(),
        )
    }

    #[test]
    fn matched_sets_are_reachable() {
        let engine = ToyEngine {
            screen: screen_with_actions(&["continue", "skip"]),
        };
        let report = check_static_reachability(&engine, &["continue", "skip"]);
        assert!(report.is_reachable(), "expected reachable, got {report:?}");
        assert_reachability(&engine, &["continue", "skip"]);
    }

    #[test]
    fn declared_handler_without_affordance_is_an_orphan_handler() {
        let engine = ToyEngine {
            screen: screen_with_actions(&["continue", "skip"]),
        };
        let report =
            check_static_reachability(&engine, &["continue", "skip", "submit_custom_group"]);
        assert!(!report.is_reachable());
        assert_eq!(
            report.orphan_handlers,
            BTreeSet::from(["submit_custom_group".to_string()])
        );
        assert!(report.orphan_affordances.is_empty());
    }

    #[test]
    fn affordance_without_handler_is_an_orphan_affordance() {
        let engine = ToyEngine {
            screen: screen_with_actions(&["continue", "skip"]),
        };
        let report = check_static_reachability(&engine, &["continue"]);
        assert!(!report.is_reachable());
        assert_eq!(
            report.orphan_affordances,
            BTreeSet::from(["skip".to_string()])
        );
        assert!(report.orphan_handlers.is_empty());
    }

    #[test]
    fn both_orphan_classes_are_reported_independently() {
        let engine = ToyEngine {
            screen: screen_with_actions(&["continue", "skip"]),
        };
        let report = check_static_reachability(&engine, &["continue", "submit_custom_group"]);
        assert!(!report.is_reachable());
        assert_eq!(
            report.orphan_handlers,
            BTreeSet::from(["submit_custom_group".to_string()])
        );
        assert_eq!(
            report.orphan_affordances,
            BTreeSet::from(["skip".to_string()])
        );
    }

    #[test]
    #[should_panic(expected = "orphan handlers")]
    fn assert_reachability_panics_with_orphan_details() {
        let engine = ToyEngine {
            screen: screen_with_actions(&["continue"]),
        };
        assert_reachability(&engine, &["continue", "submit_custom_group"]);
    }

    #[test]
    fn non_action_pressed_affordances_are_ignored_by_static_diff() {
        // A `ToggleList` or `TextInput` produces `ItemToggled` /
        // `TextChanged`, not `ActionPressed`. The static diff only
        // looks at action ids, so a screen containing such a
        // component with no `ScreenAction` is trivially reachable.
        use crate::ui::component::{Component, ToggleItem};
        let screen = ScreenModel::new(
            "toy",
            "Toy",
            vec![Component::ToggleList {
                id: "groups".into(),
                label: "Groups".into(),
                items: vec![ToggleItem {
                    id: "family".into(),
                    label: "Family".into(),
                    selected: false,
                    subtitle: None,
                    a11y: None,
                    info_key: None,
                }],
                a11y: None,
            }],
            vec![],
        );
        let engine = ToyEngine { screen };
        let report = check_static_reachability(&engine, &[]);
        assert!(report.is_reachable(), "got {report:?}");
    }
}
