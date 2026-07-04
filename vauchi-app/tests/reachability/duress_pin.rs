// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `DuressPinEngine`.
//!
//! Multi-step wizard — `duress_overview` -> `duress_enter_pin` ->
//! `duress_confirm_pin` -> `duress_alerts` — each with a distinct
//! `screen_id`. The BFS reaches **all four**: the structural walker
//! now primes `Component::PinInput` by emitting one single-char
//! `TextChanged` per slot (`screen_walker::walk_component`), filling
//! the `pin` and `confirm_pin` fields. Both fields fill to the same
//! value (`PIN_PRIMING_DIGIT`), so the empty-PIN gate on `EnterPin`'s
//! "continue" *and* the `confirm_pin == new_pin` gate on
//! `ConfirmPin`'s "continue" (`duress_pin.rs`) both clear, exposing
//! `duress_alerts` and its `save` affordance.
//!
//! Still pinned via the engine integration tests, not BFS-reachable:
//! - `confirm_disable` / `cancel_disable` — pressing `disable` flips
//!   `pending_disable` and adds an `InlineConfirm`, but that screen
//!   keeps `screen_id == "duress_overview"`, so `screen_id` dedup
//!   collapses it.
//!
//! End-to-end coverage lives in
//! `core/vauchi-core/tests/it/duress_pin_engine_tests.rs`.

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{DuressConfig, DuressPinEngine, WorkflowEngine};

/// Action ids emitted across the four BFS-reachable screens
/// (`duress_overview`, `duress_enter_pin`, `duress_confirm_pin`,
/// `duress_alerts`) and consumed by `DuressPinEngine::handle_action`
/// — `core/vauchi-app/src/ui/duress_pin.rs`. `save` is the
/// alerts-step affordance reached once the PIN gates clear.
const HANDLED: &[&str] = &["configure", "disable", "back", "continue", "save"];

fn factory() -> DuressPinEngine {
    // `enabled = true` so the `Overview` step renders the `disable`
    // affordance (it is hidden when duress is off). Empty alert
    // config — the alerts step still renders `save` / `back`.
    DuressPinEngine::new(DuressConfig {
        enabled: true,
        available_contacts: Vec::new(),
        selected_contact_ids: Vec::new(),
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
