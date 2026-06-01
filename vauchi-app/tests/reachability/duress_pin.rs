// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `DuressPinEngine`.
//!
//! Multi-step wizard - `duress_overview` -> `duress_enter_pin` ->
//! `duress_confirm_pin` -> `duress_alerts` - each with a distinct
//! `screen_id`. BFS reaches only the first two.
//!
//! The PIN fields are `Component::PinInput`, and the structural
//! walker only primes `TextInput` / `EditableText`
//! (`screen_walker::walk_component`) - it does not synthesise
//! `PinInput` entry. So the empty-PIN gate on `EnterPin`'s
//! "continue" (`duress_pin.rs:360`) never clears, and
//! `duress_confirm_pin` / `duress_alerts` stay unreachable from a
//! structural walk. The reachable affordance set is
//! `{configure, disable}` (overview, with `enabled = true`) plus
//! `{back, continue}` (enter-pin).
//!
//! Pinned elsewhere (not BFS-reachable):
//! - `duress_confirm_pin` (its `back` / `continue` share the
//!   enter-pin ids) and `duress_alerts` (`save`) - behind the
//!   PinInput gate;
//! - `confirm_disable` / `cancel_disable` - pressing `disable`
//!   flips `pending_disable` and adds an `InlineConfirm`, but that
//!   screen keeps `screen_id == "duress_overview"`, so `screen_id`
//!   dedup collapses it.
//!
//! All are exercised end-to-end by
//! `core/vauchi-core/tests/it/duress_pin_engine_tests.rs`. Declaring
//! any here would make them orphan handlers.

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{DuressConfig, DuressPinEngine, WorkflowEngine};

/// Action ids emitted across the two BFS-reachable screens
/// (`duress_overview`, `duress_enter_pin`) and consumed by
/// `DuressPinEngine::handle_action` -
/// `core/vauchi-app/src/ui/duress_pin.rs`.
const HANDLED: &[&str] = &["configure", "disable", "back", "continue"];

fn factory() -> DuressPinEngine {
    // `enabled = true` so the `Overview` step renders the `disable`
    // affordance (it is hidden when duress is off). Empty alert
    // config - the alerts step still renders `save` / `back`.
    DuressPinEngine::new(DuressConfig {
        enabled: true,
        alert_contacts: Vec::new(),
        alert_message: String::new(),
        include_location: false,
    })
}

// @internal
#[test]
fn duress_overview_screen_is_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "duress_overview");
    assert_reachability_across_screens(factory, HANDLED);
}
