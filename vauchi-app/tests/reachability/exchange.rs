// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `ExchangeEngine`.
//!
//! The initial screen when `ExchangeConfig.mode` is `None` is
//! the `exchange_mode_selection` screen rendered by
//! `ModeSelectionEngine`. That screen emits only
//! `ActionList` items (→ `UserAction::ListItemSelected`) and
//! no `ScreenAction`s — the static-diff harness compares
//! `ActionPressed` action_ids only, so today's correct state
//! is an empty walked set AND empty declared set.
//!
//! This test's main value is regression protection: if a
//! future change adds a `ScreenAction` to `exchange_mode_selection`
//! without a matching `handle_action` arm in the dispatch
//! path, the harness will flag the new affordance as orphaned.
//! Later-step handlers (`BleMode`, `Link`, `Qr`, etc.) are
//! tested in the engine's own unit tests.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{ExchangeConfig, ExchangeEngine, WorkflowEngine};

/// No `ActionPressed`-shaped handler arms exist on the initial
/// `exchange_mode_selection` screen — selections fan out through
/// `UserAction::ListItemSelected` (see `handle_action` in
/// `core/vauchi-app/src/ui/exchange_mode_selection.rs:109`).
const MODE_SELECTION_HANDLED: &[&str] = &[];

fn factory() -> ExchangeEngine {
    ExchangeEngine::new(
        ExchangeConfig {
            own_name: "Test".into(),
            own_qr_data: "v1:test".into(),
            available_groups: Vec::new(),
            device_capabilities: Default::default(),
            transport_readiness: Default::default(),
            mode: None,
            last_used_group_ids: None,
            last_used_mode: None,
            card_snapshot: None,
            available_group_data: Vec::new(),
        },
        vauchi_core::clock::SystemClock::shared(),
    )
}

// @internal
#[test]
fn exchange_initial_mode_selection_is_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "exchange_mode_selection");
    assert_reachability(&engine, MODE_SELECTION_HANDLED);
}
