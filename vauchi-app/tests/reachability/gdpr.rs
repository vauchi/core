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
//! checked by the AppEngine, / `cancel`). Once the AppEngine confirms it,
//! `export_choose_format` offers `export_encrypted` -> `export_password`
//! (`confirm_export_password` / `cancel`) and `export_vcard` /
//! `export_json` -> `export_readable_warning` (`confirm_readable_export` /
//! `cancel`).
//!
//! With someone trusted for recovery, `confirm_delete` ->
//! `delete_recovery_choice` (`keep_recovery` / `erase_everything` /
//! `cancel`) (#711).
//!
//! The `consent_actions` `ActionList` rows (`view_data` / `manage_consent`)
//! are `ListItemSelected` pass-throughs and are not walked here.

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{EngineUpdate, GdprEngine, GdprExportUpdate, UserAction, WorkflowEngine};

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

const HANDLED_FORMAT_CHOICE: &[&str] = &[
    "export_encrypted",
    "export_vcard",
    "export_json",
    "confirm_export_password",
    "confirm_readable_export",
    "export",
    "heirloom",
    "confirm_heirloom",
    "confirm_pin",
    "delete",
    "confirm_delete",
    "cancel",
];

/// Past the password step, which only the AppEngine can confirm.
fn format_choice_factory() -> GdprEngine {
    let mut engine = app_password_factory();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "export".into(),
    });
    assert!(engine.apply_update(EngineUpdate::GdprExport(GdprExportUpdate::Verified)));
    engine
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

// @internal
#[test]
fn gdpr_export_format_screens_are_reachable() {
    assert_eq!(
        format_choice_factory().current_screen().screen_id,
        "export_choose_format"
    );
    assert_reachability_across_screens(format_choice_factory, HANDLED_FORMAT_CHOICE);
}

/// No app password, but the shell can run the OS user-presence check: the
/// export waits on it (`cancel` only; the answer is a hardware event).
fn user_presence_factory() -> GdprEngine {
    factory().with_user_presence_available(true)
}

// @internal
#[test]
fn gdpr_export_presence_screen_is_reachable() {
    assert_reachability_across_screens(user_presence_factory, HANDLED_DEFAULT_PRESENCE);
}

const HANDLED_DEFAULT_PRESENCE: &[&str] = &[
    "export",
    "heirloom",
    "confirm_heirloom",
    "delete",
    "confirm_delete",
    "cancel",
];

const HANDLED_WITH_TRUSTED_CONTACTS: &[&str] = &[
    "export",
    "heirloom",
    "confirm_heirloom",
    "set_app_password",
    "delete",
    "confirm_delete",
    "keep_recovery",
    "erase_everything",
    "cancel",
];

fn trusted_contacts_factory() -> GdprEngine {
    factory().with_trusted_contacts(true)
}

// @internal
#[test]
fn gdpr_deletion_recovery_choice_is_reachable() {
    assert_reachability_across_screens(trusted_contacts_factory, HANDLED_WITH_TRUSTED_CONTACTS);
}
