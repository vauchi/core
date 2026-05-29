// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `LinkExchangeEngine`.
//!
//! Phase 3b of `2026-05-11-link-exchange-engine-graduation`. The
//! initiator flow renders five screens — share-url, waiting,
//! retrieving, success, failed — driven by the engine-owned
//! `LinkInitiatorSession` lifecycle through the engine's
//! `set_share_url` / `transition_to_*` bridge setters. CC-22 requires
//! the declared handler set match what the BFS walker emits.
//!
//! BFS coverage notes:
//! - Share-url exposes `share` (→ waiting, non-terminal) + `cancel`.
//!   From the share-url root the walker also reaches the waiting
//!   screen (its only affordance, `cancel`, is already in the set).
//! - Retrieving exposes no actions.
//! - Success exposes `done`.
//! - Failed exposes `retry` (emits the terminal `StartLinkExchange`)
//!   + `cancel`.
//!
//! Reaching the retrieving / success / failed screens from the initial
//! share-url engine requires per-state factories — the walker only
//! follows ScreenAction button presses, and the lifecycle
//! `transition_to_*` setters are bridge-side mutators. Each factory is
//! a separate BFS root, mirroring `multi_stage_exchange.rs`.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{
    LINK_EXCHANGE_ACTION_CANCEL, LINK_EXCHANGE_ACTION_DONE, LINK_EXCHANGE_ACTION_RETRY,
    LINK_EXCHANGE_ACTION_SHARE, LinkExchangeEngine, WorkflowEngine,
};

/// Share-url root: pressing `share` navigates to the waiting screen
/// (whose only affordance, `cancel`, is shared with this screen).
const SHARE_URL_HANDLED: &[&str] = &[LINK_EXCHANGE_ACTION_SHARE, LINK_EXCHANGE_ACTION_CANCEL];
/// Retrieving screen has no top-level actions.
const RETRIEVING_HANDLED: &[&str] = &[];
/// Success screen exposes only `done`.
const SUCCESS_HANDLED: &[&str] = &[LINK_EXCHANGE_ACTION_DONE];
/// Failed screen exposes `retry` + `cancel`.
const FAILED_HANDLED: &[&str] = &[LINK_EXCHANGE_ACTION_RETRY, LINK_EXCHANGE_ACTION_CANCEL];

fn share_url_factory() -> LinkExchangeEngine {
    let mut e = LinkExchangeEngine::new();
    e.set_share_url("vauchi://exchange?example".into());
    e
}

fn retrieving_factory() -> LinkExchangeEngine {
    let mut e = LinkExchangeEngine::new();
    e.transition_to_waiting();
    e.transition_to_retrieving();
    e
}

fn success_factory() -> LinkExchangeEngine {
    let mut e = LinkExchangeEngine::new();
    e.transition_to_success();
    e
}

fn failed_factory() -> LinkExchangeEngine {
    let mut e = LinkExchangeEngine::new();
    e.transition_to_failed("polling_timed_out".into());
    e
}

// @internal
#[test]
fn share_url_screen_is_reachable_with_share_and_cancel() {
    let engine = share_url_factory();
    assert_eq!(engine.current_screen().screen_id, "exchange_share_url");
    assert_reachability_across_screens(share_url_factory, SHARE_URL_HANDLED);
}

// @internal
#[test]
fn retrieving_screen_is_reachable_with_no_actions() {
    let engine = retrieving_factory();
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_link_retrieving"
    );
    assert_reachability_across_screens(retrieving_factory, RETRIEVING_HANDLED);
}

// @internal
#[test]
fn success_screen_is_reachable_with_done() {
    let engine = success_factory();
    assert_eq!(engine.current_screen().screen_id, "exchange_link_success");
    assert_reachability_across_screens(success_factory, SUCCESS_HANDLED);
}

// @internal
#[test]
fn failed_screen_is_reachable_with_retry_and_cancel() {
    let engine = failed_factory();
    assert_eq!(engine.current_screen().screen_id, "exchange_link_failed");
    assert_reachability_across_screens(failed_factory, FAILED_HANDLED);
}

// @internal
#[test]
fn no_orphans_on_share_url_screen() {
    let report = check_reachability(share_url_factory, SHARE_URL_HANDLED);
    assert!(
        report.is_reachable(),
        "share_url: unexpected orphans: {report:?}"
    );
}

// @internal
#[test]
fn no_orphans_on_retrieving_screen() {
    let report = check_reachability(retrieving_factory, RETRIEVING_HANDLED);
    assert!(
        report.is_reachable(),
        "retrieving: unexpected orphans: {report:?}"
    );
}

// @internal
#[test]
fn no_orphans_on_success_screen() {
    let report = check_reachability(success_factory, SUCCESS_HANDLED);
    assert!(
        report.is_reachable(),
        "success: unexpected orphans: {report:?}"
    );
}

// @internal
#[test]
fn no_orphans_on_failed_screen() {
    let report = check_reachability(failed_factory, FAILED_HANDLED);
    assert!(
        report.is_reachable(),
        "failed: unexpected orphans: {report:?}"
    );
}
