// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `EmergencyShredEngine`.
//!
//! Destructive two-step flow (ADR-022): `shred_warning` ->
//! `shred_confirm`. `continue` advances warning -> confirm; `wipe`
//! completes only when the `confirmation` TextInput equals the exact
//! string "DELETE", handing the wipe to the AppEngine (#431). The
//! reachable affordance set is `continue` / `cancel` (warning) plus
//! `wipe` / `cancel` (confirm).

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{EmergencyShredEngine, WorkflowEngine};

/// Action ids emitted across the two BFS-reachable screens and
/// consumed by `EmergencyShredEngine::handle_action` -
/// `core/vauchi-app/src/ui/emergency_shred.rs`.
const HANDLED: &[&str] = &["continue", "cancel", "wipe"];

fn factory() -> EmergencyShredEngine {
    EmergencyShredEngine::new(vauchi_app::i18n::Locale::English)
}

// @internal
#[test]
fn emergency_shred_screens_are_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "shred_warning");
    assert_reachability_across_screens(factory, HANDLED);
}
