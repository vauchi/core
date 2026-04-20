// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability tests for `OnboardingEngine`.
//!
//! Plan Task 1.3 Step 1. The initial screen (IdentityCheck)
//! should be reachable against the documented consumer list; a
//! failure here is either a plan-documented orphan (e.g. the
//! `submit_custom_group` defect on the later `groups_setup`
//! screen, tracked in
//! `_private/docs/problems/2026-04-20-onboarding-custom-group-add-invisible`)
//! or a real regression.

use vauchi_app::ui::testing::{assert_reachability, check_static_reachability};
use vauchi_app::ui::{OnboardingEngine, WorkflowEngine};

/// Action ids `handle_identity_check` consumes
/// (`core/vauchi-app/src/ui/onboarding.rs:512`).
const IDENTITY_CHECK_HANDLED: &[&str] = &["have_identity", "create_new"];

#[test]
fn initial_identity_check_screen_is_reachable() {
    let engine = OnboardingEngine::new();
    assert_eq!(engine.current_screen().screen_id, "identity_check");
    assert_reachability(&engine, IDENTITY_CHECK_HANDLED);
}

#[test]
fn initial_screen_affordance_set_matches_plan() {
    // Regression: pins the walker-emitted action ids on the
    // IdentityCheck screen. If this list changes, update
    // `IDENTITY_CHECK_HANDLED` in lock-step.
    let engine = OnboardingEngine::new();
    let report = check_static_reachability(&engine, IDENTITY_CHECK_HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}
