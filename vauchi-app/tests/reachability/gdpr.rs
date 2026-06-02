// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability tests for `GdprEngine`.
//!
//! The overview's deletion actions are conditional on state and mutually
//! exclusive, so two factories are needed:
//!
//! - **default** (no deletion scheduled): overview offers `export` /
//!   `delete` / `panic_shred`; `delete` -> `delete_identity_summary`
//!   (`confirm_delete` / `cancel`); `panic_shred` -> `confirm_panic_shred`
//!   (`confirm_shred` / `cancel`).
//! - **scheduled + grace elapsed**: overview offers `export` /
//!   `cancel_deletion` / `execute_deletion` / `panic_shred`;
//!   `execute_deletion` -> `confirm_execute_deletion` (`confirm_execute`
//!   / `cancel`).
//!
//! The `consent_actions` `ActionList` rows (`view_data` / `manage_consent`)
//! are `ListItemSelected` pass-throughs and are not walked here.

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{GdprEngine, WorkflowEngine};

const HANDLED_DEFAULT: &[&str] = &[
    "export",
    "delete",
    "panic_shred",
    "confirm_delete",
    "confirm_shred",
    "cancel",
];

const HANDLED_SCHEDULED: &[&str] = &[
    "export",
    "cancel_deletion",
    "execute_deletion",
    "panic_shred",
    "confirm_execute",
    "confirm_shred",
    "cancel",
];

fn factory() -> GdprEngine {
    GdprEngine::new(None, "Consent summary".into())
}

fn scheduled_factory() -> GdprEngine {
    GdprEngine::new(Some("Scheduled".into()), "Consent summary".into())
        .with_deletion_scheduled(true)
        .with_deletion_executable(true)
}

// @internal
#[test]
fn gdpr_screens_are_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "privacy_settings");
    assert_reachability_across_screens(factory, HANDLED_DEFAULT);
}

// @internal
#[test]
fn gdpr_scheduled_screens_are_reachable() {
    assert_reachability_across_screens(scheduled_factory, HANDLED_SCHEDULED);
}
