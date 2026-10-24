// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reachability test for `DuressPinEngine`.
//!
//! Multi-step wizard — `duress_overview` -> `duress_enter_pin` ->
//! `duress_confirm_pin` — each with a distinct `screen_id`. The BFS
//! reaches **all three**: the structural walker primes
//! `Component::PinInput` by emitting one single-char `TextChanged` per
//! slot (`screen_walker::walk_component`), filling the `pin` and
//! `confirm_pin` fields. Confirming completes the setup; a duress unlock
//! sends nothing, so there is no alerts step (ADR-032 amendment
//! 2026-10-11).
//!
//! The overview has no `ActionPressed` affordance (#459): Set Up PIN,
//! How duress mode looks and Turn off are body buttons, and the PIN and
//! decoy entries are rows — `ListItemSelected` pass-throughs covered by
//! `duress_screen_tests`.
//!
//! Still pinned via the engine integration tests, not BFS-reachable:
//! - `confirm_disable` / `cancel_disable` — Turn off flips
//!   `pending_disable` and adds an `InlineConfirm`, but that screen
//!   keeps `screen_id == "duress_overview"`, so `screen_id` dedup
//!   collapses it.
//!
//! End-to-end coverage lives in
//! `core/vauchi-core/tests/it/duress_pin_engine_tests.rs`.

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{DuressConfig, DuressPinEngine, WorkflowEngine};

/// Action ids emitted across the three BFS-reachable screens
/// (`duress_overview`, `duress_enter_pin`, `duress_confirm_pin`) and
/// consumed by `DuressPinEngine::handle_action` —
/// `core/vauchi-app/src/ui/duress_pin.rs`.
const HANDLED: &[&str] = &["back", "continue"];

fn factory() -> DuressPinEngine {
    // `enabled = true` so the overview renders its management rows.
    DuressPinEngine::new(
        DuressConfig { enabled: true },
        vauchi_app::i18n::Locale::ENGLISH,
    )
}

// @internal
#[test]
fn duress_overview_screen_is_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "duress_overview");
    assert_reachability_across_screens(factory, HANDLED);
}
