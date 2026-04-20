// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability tests for `OnboardingEngine`.
//!
//! Plan Task 1.3 Step 1. Uses the multi-screen BFS so this test
//! exercises the full engine, not just the initial IdentityCheck
//! screen. Today there are three documented orphans; each line
//! in the expected sets carries a problem-record pointer.
//!
//! When any of those orphans is fixed, this test must be updated
//! in the same change — the assertion deliberately pins the
//! current defect state.

use std::collections::BTreeSet;

use vauchi_app::ui::testing::{assert_reachability, check_reachability, check_static_reachability};
use vauchi_app::ui::{OnboardingEngine, WorkflowEngine};

/// Action ids `handle_identity_check` consumes
/// (`core/vauchi-app/src/ui/onboarding.rs:512`).
const IDENTITY_CHECK_HANDLED: &[&str] = &["have_identity", "create_new"];

/// Union of action ids consumed by every `handle_*` in
/// `OnboardingEngine` — `handle_identity_check`,
/// `handle_link_choice`, `handle_default_name`,
/// `handle_groups_setup`, `handle_contact_info`, and
/// `handle_what_next` (file: `core/vauchi-app/src/ui/onboarding.rs`).
const ONBOARDING_ALL_HANDLED: &[&str] = &[
    // identity_check
    "have_identity",
    "create_new",
    // link_choice
    "transfer_device",
    "link_device",
    "restore_backup",
    "back",
    // default_name + groups_setup + contact_info + what_next (shared)
    "continue",
    "skip",
    // default_name
    "submit_display_name",
    // groups_setup
    "submit_custom_group",
    // contact_info
    "show_phone",
    "show_email",
    // what_next
    "exchange",
    "import_contacts",
    "read_security",
    "read_backup",
    "start_app",
];

#[test]
fn initial_identity_check_screen_is_reachable() {
    let engine = OnboardingEngine::new();
    assert_eq!(engine.current_screen().screen_id, "identity_check");
    assert_reachability(&engine, IDENTITY_CHECK_HANDLED);
}

#[test]
fn initial_screen_affordance_set_matches_plan() {
    let engine = OnboardingEngine::new();
    let report = check_static_reachability(&engine, IDENTITY_CHECK_HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}

/// BFS reachability check against the full handler set today
/// reports three documented orphans on the main onboarding flow.
/// Pinning them here so any fix that lands without updating this
/// test fails loudly.
///
/// The screen set that the BFS currently reaches is:
/// `identity_check` (entry), `default_name` (via `create_new`),
/// `groups_setup` (via `continue` on default_name with
/// a non-empty name), `contact_info`, `what_next`, and
/// `link_choice` (via `have_identity`). Six screens.
///
/// The orphans:
/// - `submit_display_name` — handler arm at
///   `onboarding.rs:551`, no `ScreenAction` in `build_default_name`
///   (`onboarding.rs:263`) emits that id. Pressing "Continue" on
///   default_name routes through the shared `continue` branch
///   instead. Documented indirectly in
///   `_private/docs/problems/2026-04-20-frontend-correctness-strategy/`
///   as a Layer 1 false positive.
/// - `submit_custom_group` — handler arm at `onboarding.rs:606`,
///   no `ScreenAction` in `build_groups_setup` (`onboarding.rs:344`)
///   emits that id. Record:
///   `_private/docs/problems/2026-04-20-onboarding-custom-group-add-invisible`.
/// - `add_social` — `ScreenAction` in `build_contact_info`
///   (`onboarding.rs:432`) but no handler arm in
///   `handle_contact_info` consumes it; tap is a silent no-op.
///   No dedicated record yet — this test is its initial landing
///   ground.
#[test]
fn bfs_pins_three_known_orphans() {
    let report = check_reachability(OnboardingEngine::new, ONBOARDING_ALL_HANDLED);

    assert_eq!(
        report.orphan_handlers,
        BTreeSet::from([
            "submit_custom_group".to_string(),
            "submit_display_name".to_string(),
        ]),
        "orphan handler set drifted — if a fix landed, remove the\n\
         corresponding id from the expected set and (if applicable)\n\
         from ONBOARDING_ALL_HANDLED."
    );

    assert_eq!(
        report.orphan_affordances,
        BTreeSet::from(["add_social".to_string()]),
        "orphan affordance set drifted — if `add_social` now has a\n\
         handler, remove it from this expected set."
    );
}

/// Guards the BFS itself: the full flow should reach six distinct
/// screens. If this drops, the BFS regressed or a screen
/// disappeared silently.
#[test]
fn bfs_reaches_all_six_onboarding_screens() {
    use vauchi_app::ui::testing::all_reachable_screens;
    let screens = all_reachable_screens(OnboardingEngine::new);
    let ids: BTreeSet<String> = screens.into_iter().map(|s| s.screen_id).collect();
    assert_eq!(
        ids,
        BTreeSet::from([
            "identity_check".to_string(),
            "link_choice".to_string(),
            "default_name".to_string(),
            "groups_setup".to_string(),
            "contact_info".to_string(),
            "what_next".to_string(),
        ]),
        "BFS screen set drifted"
    );
}
