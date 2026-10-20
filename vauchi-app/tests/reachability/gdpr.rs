// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability tests for `GdprEngine`.
//!
//! The overview's deletion actions are conditional on state and mutually
//! exclusive, so two factories are needed:
//!
//! - **default** (no deletion scheduled): overview offers `export` /
//!   `delete`; `delete` -> `delete_identity_summary`
//!   (`confirm_delete` / `cancel`).
//! - **scheduled + grace elapsed**: overview offers `export` /
//!   `cancel_deletion` / `execute_deletion`;
//!   `execute_deletion` -> `confirm_execute_deletion` (`confirm_execute`
//!   / `cancel`).
//!
//! Both states also offer `heirloom` -> `confirm_heirloom` (`confirm_heirloom`
//! / `cancel`), the paper heirloom export (private#363).
//!
//! Every export first asks for the session password (#712): without an app
//! password `export_needs_app_password` (`set_app_password`, handled by the
//! AppEngine, / `cancel`); with one `export_confirm_pin` (`confirm_pin`,
//! checked by the AppEngine, / `cancel`).
//!
//! The `consent_actions` `ActionList` rows (`view_data` / `manage_consent`)
//! are `ListItemSelected` pass-throughs and are not walked here.

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{GdprEngine, WorkflowEngine};

const HANDLED_DEFAULT: &[&str] = &[
    "export",
    "heirloom",
    "confirm_heirloom",
    "set_app_password",
    "delete",
    "confirm_delete",
    "cancel",
];

const HANDLED_WITH_APP_PASSWORD: &[&str] = &[
    "export",
    "heirloom",
    "confirm_heirloom",
    "confirm_pin",
    "delete",
    "confirm_delete",
    "cancel",
];

const HANDLED_SCHEDULED: &[&str] = &[
    "export",
    "heirloom",
    "confirm_heirloom",
    "cancel_deletion",
    "execute_deletion",
    "confirm_execute",
    "set_app_password",
    "cancel",
];

fn factory() -> GdprEngine {
    GdprEngine::new(
        None,
        "Consent summary".into(),
        vauchi_app::i18n::Locale::English,
    )
}

fn app_password_factory() -> GdprEngine {
    factory().with_app_password_set(true)
}

fn scheduled_factory() -> GdprEngine {
    GdprEngine::new(
        Some("Scheduled".into()),
        "Consent summary".into(),
        vauchi_app::i18n::Locale::English,
    )
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

// @internal
#[test]
fn gdpr_export_pin_screen_is_reachable() {
    assert_reachability_across_screens(app_password_factory, HANDLED_WITH_APP_PASSWORD);
}
