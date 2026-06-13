// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `ChangePasswordEngine`.
//!
//! Single-screen rotation form with three masked inputs and a
//! `submit` / `cancel` action pair. Both action ids are consumed by
//! the engine: `submit` only fires when [`ChangePasswordEngine`]'s
//! validation enables it, and the actual storage rotation lives in
//! `AppEngine::handle_completion` for `AppScreen::ChangePassword`.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{ChangePasswordEngine, WorkflowEngine};

/// Action ids handled by `ChangePasswordEngine` —
/// `core/vauchi-app/src/ui/change_password.rs`.
///
/// `submit` starts disabled (validation gate), so the BFS walker
/// can't reach it without flipping the engine into the validated
/// state. Declare it as handled so reachability is satisfied — the
/// integration tests in
/// `core/vauchi-core/tests/it/app_engine_settings_lock_tests.rs`
/// cover the enabled path end-to-end.
const HANDLED: &[&str] = &["submit", "cancel"];

fn factory() -> ChangePasswordEngine {
    ChangePasswordEngine::new(true)
}

/// Setup mode (no password configured) — same `submit`/`cancel` action ids,
/// but a different 2-field component tree the BFS walker must also cover.
fn factory_setup() -> ChangePasswordEngine {
    ChangePasswordEngine::new(false)
}

// @internal
#[test]
fn change_password_screen_is_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "change_password");
    assert_reachability_across_screens(factory, HANDLED);
}

// @internal
#[test]
fn change_password_has_no_orphans() {
    let report = check_reachability(factory, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}

// @internal
#[test]
fn set_password_setup_mode_is_reachable_and_orphan_free() {
    let engine = factory_setup();
    assert_eq!(engine.current_screen().screen_id, "change_password");
    assert_eq!(engine.current_screen().title, "Set Password");
    assert_reachability_across_screens(factory_setup, HANDLED);
    let report = check_reachability(factory_setup, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}
