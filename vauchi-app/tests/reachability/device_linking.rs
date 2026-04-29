// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `DeviceLinkingEngine`.
//!
//! Pair 5 of the Pure Humble UI retirement work
//! (`_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens/`).
//!
//! BFS coverage notes:
//! - The transport-selection root reaches `TransportSelection`,
//!   `OfflineStub`, and `ShowQr` via button presses (`select_internet`,
//!   `select_offline`, `back_to_transport`, `cancel`).
//! - `VerifyCode` is only reachable via the bridge mutator
//!   `peer_connected` — declared via a per-state factory.
//! - The receiver-side `ConfirmingDevice → VerifyingProximity` cluster
//!   is only reachable via the bridge mutator
//!   `transition_to_confirming_device` — declared via a per-state
//!   factory. `codes_match` then transitions to `VerifyingProximity`
//!   inside BFS.
//! - The `QrPending`, `WaitingForRequest`, `QrExpired`, `Completing`,
//!   `LinkSuccess`, and `LinkFailed` clusters each get their own
//!   factory because the BFS walker can't drive bridge mutators.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{DeviceLinkingEngine, WorkflowEngine};

const TRANSPORT_HANDLED: &[&str] = &[
    "select_internet",
    "select_offline",
    "back_to_transport",
    "cancel",
];

// `reject` from VerifyCode → ShowQr, which emits `cancel`. BFS reaches
// both, so the declared set must include cancel.
const VERIFY_CODE_HANDLED: &[&str] = &["confirm", "reject", "cancel"];

const CONFIRMING_HANDLED: &[&str] = &["codes_match", "deny", "confirm_manual", "cancel"];

const QR_PENDING_HANDLED: &[&str] = &["cancel"];
const WAITING_HANDLED: &[&str] = &["cancel"];
const QR_EXPIRED_HANDLED: &[&str] = &["retry", "cancel"];
const COMPLETING_HANDLED: &[&str] = &["cancel"];
const SUCCESS_HANDLED: &[&str] = &["done"];
const FAILED_HANDLED: &[&str] = &["retry", "cancel"];

fn transport_factory() -> DeviceLinkingEngine {
    DeviceLinkingEngine::with_transport_selection("qr-data".into())
}

fn verify_code_factory() -> DeviceLinkingEngine {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.peer_connected("123456".into());
    e
}

fn confirming_factory() -> DeviceLinkingEngine {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_confirming_device("New iPad".into(), "123456".into(), "deadbeef".into());
    e
}

fn qr_pending_factory() -> DeviceLinkingEngine {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_qr_pending();
    e
}

fn waiting_factory() -> DeviceLinkingEngine {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_waiting_for_request("qr-data".into(), 1_700_000_000);
    e
}

fn qr_expired_factory() -> DeviceLinkingEngine {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_qr_expired();
    e
}

fn completing_factory() -> DeviceLinkingEngine {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_confirming_device("New iPad".into(), "123456".into(), "deadbeef".into());
    e.transition_to_completing();
    e
}

fn success_factory() -> DeviceLinkingEngine {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_link_success();
    e
}

fn failed_factory() -> DeviceLinkingEngine {
    let mut e = DeviceLinkingEngine::new("qr-data".into());
    e.transition_to_link_failed("relay unreachable".into());
    e
}

// @internal
#[test]
fn transport_screen_is_reachable() {
    let e = transport_factory();
    assert_eq!(e.current_screen().screen_id, "link_transport");
    assert_reachability_across_screens(transport_factory, TRANSPORT_HANDLED);
}

// @internal
#[test]
fn verify_code_screen_is_reachable() {
    let e = verify_code_factory();
    assert_eq!(e.current_screen().screen_id, "link_verify");
    assert_reachability_across_screens(verify_code_factory, VERIFY_CODE_HANDLED);
}

// @internal
#[test]
fn confirming_device_cluster_is_reachable() {
    let e = confirming_factory();
    assert_eq!(e.current_screen().screen_id, "link_confirming_device");
    assert_reachability_across_screens(confirming_factory, CONFIRMING_HANDLED);
}

// @internal
#[test]
fn qr_pending_screen_is_reachable() {
    let e = qr_pending_factory();
    assert_eq!(e.current_screen().screen_id, "link_qr_pending");
    assert_reachability_across_screens(qr_pending_factory, QR_PENDING_HANDLED);
}

// @internal
#[test]
fn waiting_for_request_screen_is_reachable() {
    let e = waiting_factory();
    assert_eq!(e.current_screen().screen_id, "link_waiting");
    assert_reachability_across_screens(waiting_factory, WAITING_HANDLED);
}

// @internal
#[test]
fn qr_expired_screen_is_reachable() {
    let e = qr_expired_factory();
    assert_eq!(e.current_screen().screen_id, "link_qr_expired");
    assert_reachability_across_screens(qr_expired_factory, QR_EXPIRED_HANDLED);
}

// @internal
#[test]
fn completing_screen_is_reachable() {
    let e = completing_factory();
    assert_eq!(e.current_screen().screen_id, "link_completing");
    assert_reachability_across_screens(completing_factory, COMPLETING_HANDLED);
}

// @internal
#[test]
fn success_screen_is_reachable() {
    let e = success_factory();
    assert_eq!(e.current_screen().screen_id, "link_complete");
    assert_reachability_across_screens(success_factory, SUCCESS_HANDLED);
}

// @internal
#[test]
fn failed_screen_is_reachable() {
    let e = failed_factory();
    assert_eq!(e.current_screen().screen_id, "link_failed");
    assert_reachability_across_screens(failed_factory, FAILED_HANDLED);
}

// @internal
#[test]
fn no_orphan_handlers_or_affordances_on_transport_cluster() {
    let report = check_reachability(transport_factory, TRANSPORT_HANDLED);
    assert!(report.is_reachable(), "transport: orphans: {report:?}");
}

// @internal
#[test]
fn no_orphan_handlers_or_affordances_on_confirming_cluster() {
    let report = check_reachability(confirming_factory, CONFIRMING_HANDLED);
    assert!(report.is_reachable(), "confirming: orphans: {report:?}");
}

// @internal
#[test]
fn no_orphan_handlers_or_affordances_on_failed_screen() {
    let report = check_reachability(failed_factory, FAILED_HANDLED);
    assert!(report.is_reachable(), "failed: orphans: {report:?}");
}

// @internal
#[test]
fn no_orphan_handlers_or_affordances_on_success_screen() {
    let report = check_reachability(success_factory, SUCCESS_HANDLED);
    assert!(report.is_reachable(), "success: orphans: {report:?}");
}

// @internal
#[test]
fn no_orphan_handlers_or_affordances_on_qr_expired_screen() {
    let report = check_reachability(qr_expired_factory, QR_EXPIRED_HANDLED);
    assert!(report.is_reachable(), "qr_expired: orphans: {report:?}");
}
