// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `GdprEngine`.
//!
//! Two-screen flow with distinct screen_ids: `privacy_settings`
//! (Overview) -> `delete_identity_summary` (ConfirmDelete), reached
//! by pressing `delete`. The `consent_actions` `ActionList`
//! (`view_data` / `manage_consent`) rows are `ListItemSelected`
//! pass-throughs. The reachable `ActionPressed` set is `export` /
//! `delete` (overview) plus `confirm_delete` / `cancel` (summary).

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{GdprEngine, WorkflowEngine};

const HANDLED: &[&str] = &["export", "delete", "confirm_delete", "cancel"];

fn factory() -> GdprEngine {
    GdprEngine::new(None, "Consent summary".into())
}

// @internal
#[test]
fn gdpr_screens_are_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "privacy_settings");
    assert_reachability_across_screens(factory, HANDLED);
}
