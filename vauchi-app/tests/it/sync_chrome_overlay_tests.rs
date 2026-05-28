// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for AppEngine sync-chrome overlay
//! (`apply_sync_chrome_overlay`).
//!
//! Design: `_private/docs/designs/2026-05-28-sync-chrome-overlay-design.md`.
//! Replaces iOS `HomeView.SyncStatusIndicator` per G1 of
//! `2026-05-02-ios-humble-ui-deep-retirement`.

use vauchi_app::ui::{
    ActionResult, AppEngine, Component, IndicatorKind, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;

fn test_engine() -> AppEngine {
    let vauchi = Vauchi::in_memory().unwrap();
    AppEngine::new(vauchi)
}

fn find_sync_indicator(
    components: &[Component],
) -> Option<(&str, &IndicatorKind, &Option<String>)> {
    components.iter().find_map(|c| match c {
        Component::Indicator {
            id,
            label,
            kind,
            action_id,
            ..
        } if id == "sync" => Some((label.as_str(), kind, action_id)),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// Default Idle state → Neutral sync chip on every emitted screen
// ---------------------------------------------------------------------------

// @internal
#[test]
fn idle_status_emits_neutral_sync_indicator_with_tap_action() {
    // On engine boot, sync_chrome_status defaults to Idle, so the
    // overlay should inject a Component::Indicator with kind=Neutral,
    // label="Sync", action_id=Some("sync_now") on every emitted
    // top-level screen.
    let engine = test_engine();
    let screen = engine.current_screen();
    let (label, kind, action_id) =
        find_sync_indicator(&screen.components).expect("sync indicator missing on idle screen");
    assert_eq!(label, "Sync");
    assert_eq!(*kind, IndicatorKind::Neutral);
    assert_eq!(action_id.as_deref(), Some("sync_now"));
}

// @internal
#[test]
fn sync_indicator_appears_first_in_components() {
    // The overlay inserts at index 0 so chrome stays at the top of
    // the screen body — frontends with a chrome region (toolbar)
    // can render it there, frontends without render it inline.
    let engine = test_engine();
    let screen = engine.current_screen();
    let first = screen.components.first().expect("no components");
    assert!(
        matches!(first, Component::Indicator { id, .. } if id == "sync"),
        "expected Indicator id=\"sync\" at index 0, got {first:?}"
    );
}

// ---------------------------------------------------------------------------
// Offline → overlay skipped (apply_offline_overlay Banner handles it)
// ---------------------------------------------------------------------------

// @internal
#[test]
fn offline_skips_sync_chrome_overlay() {
    // The offline overlay already injects a Component::Banner with
    // "You're offline. Changes will sync when you reconnect." — adding
    // a sync chip on top would be redundant and visually noisy.
    // apply_sync_chrome_overlay early-returns when network_online is
    // false.
    let mut engine = test_engine();
    engine.set_network_online(false);
    let screen = engine.current_screen();
    assert!(
        find_sync_indicator(&screen.components).is_none(),
        "sync indicator should not be emitted while offline"
    );
}

// @internal
#[test]
fn going_back_online_re_enables_sync_chrome_overlay() {
    // Flipping back to online restores the chip — the state read
    // is lazy, so toggling network_online flips the emission on the
    // next current_screen() call without any explicit re-init.
    let mut engine = test_engine();
    engine.set_network_online(false);
    assert!(find_sync_indicator(&engine.current_screen().components).is_none());

    engine.set_network_online(true);
    assert!(
        find_sync_indicator(&engine.current_screen().components).is_some(),
        "sync indicator should re-emit after network_online flips back to true"
    );
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------

// @internal
#[test]
fn apply_sync_chrome_overlay_is_idempotent_across_renders() {
    // Calling current_screen() multiple times in a row must not
    // accumulate sync indicators. Each emission walks the existing
    // components and skips if any Indicator with id="sync" is
    // already present — mirroring the four sibling overlays.
    let engine = test_engine();
    let screen_a = engine.current_screen();
    let screen_b = engine.current_screen();
    let count_a = screen_a
        .components
        .iter()
        .filter(|c| matches!(c, Component::Indicator { id, .. } if id == "sync"))
        .count();
    let count_b = screen_b
        .components
        .iter()
        .filter(|c| matches!(c, Component::Indicator { id, .. } if id == "sync"))
        .count();
    assert_eq!(count_a, 1, "first render emits exactly one sync indicator");
    assert_eq!(
        count_b, 1,
        "second render also emits exactly one (idempotent)"
    );
}

// ---------------------------------------------------------------------------
// sync_now action — handler responds, returns UpdateScreen
// ---------------------------------------------------------------------------

// @internal
#[test]
fn sync_now_action_returns_update_screen() {
    // The chrome chip's action_id="sync_now" must reach a real
    // AppEngine handler arm (no orphan affordance per CC-22).
    // In non-network builds the arm is a no-op other than refreshing
    // the screen; in network builds it calls Vauchi::sync() and
    // updates sync_chrome_status. Either way, the result is
    // UpdateScreen — never NoHandler or panicking.
    let mut engine = test_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "sync_now".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "sync_now must return ActionResult::UpdateScreen, got {result:?}"
    );
}

// @internal
#[test]
fn sync_now_action_pressed_does_not_panic() {
    // Smoke test: the sync_now handler arm must not panic, even
    // when there is no identity and no relay key cached. In test
    // builds without network-http, the body is a no-op. In network
    // builds, Vauchi::sync() returns NoIdentity / NotConnected
    // outcomes which the arm leaves the state unchanged for. Either
    // way the call completes and the engine emits a fresh ScreenModel.
    let mut engine = test_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "sync_now".into(),
    });
    // Second call to confirm we can still emit a screen afterward.
    let screen = engine.current_screen();
    assert!(
        !screen.components.is_empty(),
        "engine still emits a populated screen after sync_now"
    );
}
