// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `ContactVisibilityEngine`.
//!
//! Single-screen engine (`contact_visibility`): a `ToggleList` of
//! per-field visibility switches plus a `save` action. The toggles
//! are `ItemToggled` pass-throughs (not part of the reachability
//! affordance set); the only `ActionPressed` id is `save`, consumed
//! by `ContactVisibilityEngine::handle_action`
//! (`core/vauchi-app/src/ui/contact_visibility.rs`).

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{ContactVisibilityEngine, WorkflowEngine};

const HANDLED: &[&str] = &["save"];

fn engine() -> ContactVisibilityEngine {
    // Empty field set: the `save` affordance renders regardless of how
    // many toggles the list carries.
    ContactVisibilityEngine::new("Alice".into(), Vec::new())
}

// @internal
#[test]
fn contact_visibility_screen_is_reachable() {
    let engine = engine();
    assert_eq!(engine.current_screen().screen_id, "contact_visibility");
    assert_reachability(&engine, HANDLED);
}
