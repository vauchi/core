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

/// Union of action ids reachable from `OnboardingEngine`'s
/// rendered screens. Most are consumed by one of the six
/// `handle_*` arms in `core/vauchi-app/src/ui/onboarding.rs`;
/// a few are intercepted at the `AppEngine` layer BEFORE they
/// reach `OnboardingEngine::handle_action` — those are still
/// "handled" from the user's perspective and belong in this set
/// so the Layer 1 reachability diff doesn't false-positive.
const ONBOARDING_ALL_HANDLED: &[&str] = &[
    // identity_check
    "have_identity",
    "create_new",
    // link_choice (M5 B1: the two indistinguishable device-link buttons
    // merged into one that routes to device_link_guidance)
    "add_another_device",
    "restore_backup",
    "back",
    // backup_password_entry — reached only via the file-picker
    // hardware-event path (`AppEngine::handle_file_picked` calls
    // `OnboardingEngine::set_pending_backup_bytes`), which the BFS
    // does not simulate. Declared here so CC-22's "every handler
    // arm has an entry" rule holds. Phase 2B of
    // `2026-05-03-core-file-picker-command`.
    "submit_backup_password",
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
    // contact_info — intercepted by
    // `AppEngine::intercept_add_field` (app_engine/intercept.rs:159),
    // which opens the AddField `FormDialog` before the action
    // reaches `handle_contact_info`.
    "add_social",
    // what_next
    "exchange",
    "import_contacts",
    "start_app",
];

// @internal
#[test]
fn initial_identity_check_screen_is_reachable() {
    let engine = OnboardingEngine::new();
    assert_eq!(engine.current_screen().screen_id, "identity_check");
    assert_reachability(&engine, IDENTITY_CHECK_HANDLED);
}

// @internal
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
/// Two remaining orphans:
/// - `submit_display_name` — handler arm at
///   `onboarding.rs:551`, no `ScreenAction` in `build_default_name`
///   (`onboarding.rs:263`) emits that id. Pressing "Continue" on
///   default_name routes through the shared `continue` branch
///   instead. Dead code; separate tidy MR will remove.
/// - `submit_backup_password` — handler arm in
///   `handle_backup_password_entry`, only reached via the file-picker
///   hardware-event path (`AppEngine::handle_file_picked` calls
///   `OnboardingEngine::set_pending_backup_bytes` which transitions to
///   `Step::BackupPasswordEntry`). The BFS walker doesn't simulate
///   `Event::FilePickedFromUser`, so the screen + its
///   "Restore" affordance never appear in the BFS reach. Coverage of
///   the live submit path lives in `tests/it/file_picker_wiring_tests.rs`
///   (`submit_valid_password_imports_backup_and_navigates_to_main`,
///   `submit_wrong_password_returns_alert_and_clears_state`). Phase 2B
///   of `2026-05-03-core-file-picker-command`.
///
/// Flipped 2026-04-21 (session #1): `submit_custom_group` was an
/// orphan handler until `build_groups_setup` gained an "Add group"
/// `ScreenAction` (core !634). First observed L1 regression-gate
/// flip for a user-facing fix.
///
/// Flipped 2026-04-21 (session #2): `add_social` was an apparent
/// orphan AFFORDANCE until this commit added it to the declared
/// set. Audit revealed `AppEngine::intercept_add_field`
/// (`app_engine/intercept.rs:159`) already opens the AddField
/// `FormDialog` on tap — a Layer 1 false-positive, not a user
/// bug. Documenting the interception path in the declared set
/// keeps the harness honest.
// @internal
#[test]
fn bfs_pins_remaining_orphans() {
    let report = check_reachability(OnboardingEngine::new, ONBOARDING_ALL_HANDLED);

    assert_eq!(
        report.orphan_handlers,
        BTreeSet::from([
            "submit_display_name".to_string(),
            "submit_backup_password".to_string(),
        ]),
        "orphan handler set drifted — if a fix landed, remove the\n\
         corresponding id from the expected set and (if applicable)\n\
         from ONBOARDING_ALL_HANDLED."
    );

    assert!(
        report.orphan_affordances.is_empty(),
        "orphan affordance set drifted — if a new unhandled\n\
         `ScreenAction` appeared, either wire it to a handler\n\
         arm (in `onboarding.rs` or via `AppEngine::intercept_*`)\n\
         or add it to ONBOARDING_ALL_HANDLED with a note.\n\
         Got: {:?}",
        report.orphan_affordances
    );
}

/// Guards the BFS itself: the full flow should reach seven distinct
/// screens. If this drops, the BFS regressed or a screen
/// disappeared silently. (M5 B1 added `device_link_guidance`, reached
/// from `link_choice` via `add_another_device`.)
// @internal
#[test]
fn bfs_reaches_all_onboarding_screens() {
    use vauchi_app::ui::testing::all_reachable_screens;
    let screens = all_reachable_screens(OnboardingEngine::new);
    let ids: BTreeSet<String> = screens.into_iter().map(|s| s.screen_id).collect();
    assert_eq!(
        ids,
        BTreeSet::from([
            "identity_check".to_string(),
            "link_choice".to_string(),
            "device_link_guidance".to_string(),
            "default_name".to_string(),
            "groups_setup".to_string(),
            "contact_info".to_string(),
            "what_next".to_string(),
        ]),
        "BFS screen set drifted"
    );
}
