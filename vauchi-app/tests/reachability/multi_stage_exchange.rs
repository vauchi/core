// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `MultiStageExchangeEngine`.
//!
//! Pair 4 of `_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens`.
//! Declares the engine handler set and asserts every screen reachable
//! via the per-state ScreenModel matches that set (no orphan handlers,
//! no orphan affordances).
//!
//! BFS coverage notes:
//! - Active rendering exposes `cancel`, `switch_camera`.
//! - The Failed-state screen exposes `retry`, `cancel`.
//! - The success-state screen (Finalized + session_ended) exposes `done`.
//! - The permission-denied gate screen exposes
//!   `grant_camera_permission` and `cancel`.
//!
//! Reaching the success / failed / permission screens from the initial
//! `Idle` engine requires factory variants — the BFS walker only follows
//! ScreenAction button presses, and `set_state` is a bridge-side mutator.
//! We declare per-state factories so each screen-specific affordance is
//! reachable from at least one BFS root.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{MultiStageExchangeEngine, WorkflowEngine};
use vauchi_core::exchange::{ExchangeHardwareEvent, ProtocolState};

/// Action ids handled by `MultiStageExchangeEngine` —
/// `core/vauchi-app/src/ui/multi_stage_exchange.rs`.
const ACTIVE_HANDLED: &[&str] = &["cancel", "switch_camera"];
const FAILED_HANDLED: &[&str] = &["retry", "cancel"];
const SUCCESS_HANDLED: &[&str] = &["done"];
const PERMISSION_HANDLED: &[&str] = &["grant_camera_permission", "cancel"];

fn idle_factory() -> MultiStageExchangeEngine {
    MultiStageExchangeEngine::new()
}

fn failed_factory() -> MultiStageExchangeEngine {
    let mut e = MultiStageExchangeEngine::new();
    e.set_state(ProtocolState::Failed("timeout".into()));
    e
}

fn success_factory() -> MultiStageExchangeEngine {
    let mut e = MultiStageExchangeEngine::new();
    e.set_state(ProtocolState::Finalized);
    e.set_finalized("Alice".into());
    e.set_session_ended();
    e
}

fn permission_denied_factory() -> MultiStageExchangeEngine {
    let mut e = MultiStageExchangeEngine::new();
    e.handle_hardware_event(ExchangeHardwareEvent::PermissionDenied {
        transport: "camera".into(),
    });
    e
}

// @internal
#[test]
fn idle_screen_is_reachable_with_active_affordances() {
    let engine = idle_factory();
    assert_eq!(engine.current_screen().screen_id, "multi_stage_exchange");
    assert_reachability_across_screens(idle_factory, ACTIVE_HANDLED);
}

// @internal
#[test]
fn failed_screen_is_reachable_with_retry_and_cancel() {
    assert_reachability_across_screens(failed_factory, FAILED_HANDLED);
}

// @internal
#[test]
fn success_screen_is_reachable_with_done() {
    assert_reachability_across_screens(success_factory, SUCCESS_HANDLED);
}

// @internal
#[test]
fn permission_denied_screen_is_reachable_with_grant_and_cancel() {
    assert_reachability_across_screens(permission_denied_factory, PERMISSION_HANDLED);
}

// @internal
#[test]
fn no_orphan_handlers_or_affordances_on_idle_screen() {
    let report = check_reachability(idle_factory, ACTIVE_HANDLED);
    assert!(
        report.is_reachable(),
        "idle: unexpected orphans: {report:?}"
    );
}

// @internal
#[test]
fn no_orphan_handlers_or_affordances_on_failed_screen() {
    let report = check_reachability(failed_factory, FAILED_HANDLED);
    assert!(
        report.is_reachable(),
        "failed: unexpected orphans: {report:?}",
    );
}

// @internal
#[test]
fn no_orphan_handlers_or_affordances_on_success_screen() {
    let report = check_reachability(success_factory, SUCCESS_HANDLED);
    assert!(
        report.is_reachable(),
        "success: unexpected orphans: {report:?}",
    );
}

// @internal
#[test]
fn no_orphan_handlers_or_affordances_on_permission_screen() {
    let report = check_reachability(permission_denied_factory, PERMISSION_HANDLED);
    assert!(
        report.is_reachable(),
        "permission: unexpected orphans: {report:?}",
    );
}
