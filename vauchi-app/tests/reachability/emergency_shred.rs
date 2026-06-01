// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `EmergencyShredEngine`.
//!
//! Destructive multi-step flow (ADR-022) with distinct screen_ids:
//! `shred_warning` -> `shred_confirm` -> `shred_wiping` ->
//! `shred_complete`. BFS reaches the first two:
//!
//! `continue` advances warning -> confirm; `wipe` only advances when
//! the `confirmation` TextInput equals the exact string "DELETE"
//! (`emergency_shred.rs:231`). The structural walker primes the field
//! with `PLACEHOLDER_TEXT` ("x"), which fails that guard, so
//! `shred_wiping` / `shred_complete` stay unreachable from a
//! structural walk. The reachable affordance set is
//! `continue` / `cancel` (warning) plus `wipe` / `cancel` (confirm).
//!
//! Pinned elsewhere: `done` (`shred_complete`) - reached only after
//! the `processing_complete` hardware callback flips wiping -> complete
//! (`emergency_shred.rs:40`). Covered by the engine's inline tests.
//! Declaring it here would make it an orphan handler.

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{EmergencyShredEngine, WorkflowEngine};

/// Action ids emitted across the two BFS-reachable screens and
/// consumed by `EmergencyShredEngine::handle_action` -
/// `core/vauchi-app/src/ui/emergency_shred.rs`.
const HANDLED: &[&str] = &["continue", "cancel", "wipe"];

fn factory() -> EmergencyShredEngine {
    EmergencyShredEngine::new()
}

// @internal
#[test]
fn emergency_shred_screens_are_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "shred_warning");
    assert_reachability_across_screens(factory, HANDLED);
}
