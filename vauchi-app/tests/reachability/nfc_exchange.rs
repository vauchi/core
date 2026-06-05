// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `NfcExchangeEngine` (CC-22).
//!
//! NFC graduation: the dedicated engine wraps `NfcExchangeFlow` and opens on
//! the Send/Receive role chooser. CC-22 requires the declared handler set
//! match what the BFS walker emits.
//!
//! BFS coverage notes:
//! - The role chooser (`exchange_nfc_role`) exposes a `cancel` `ScreenAction`
//!   plus two `ActionList` items (`nfc_role:send` / `nfc_role:receive`). The
//!   walker treats `ActionList` items as *navigation edges*, not affordances,
//!   so the declared handler set is just `cancel`; picking either role is a
//!   navigation to the awaiting-tap holding screen
//!   (`exchange_nfc_awaiting_tap`), whose only affordance — `cancel` — is the
//!   same id, so the role-root walk covers both screens.
//! - The in-progress / verifying screens and Success are driven by the NFC
//!   *hardware* handshake (events, not actions) and so are unreachable by the
//!   action walker; they expose only `cancel` (in-progress) or the shared
//!   `done` (Success — identical to the BLE/Link success screen already
//!   covered). The failed screen is reached via a no-identity Send.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{
    NFC_EXCHANGE_ACTION_CANCEL, NFC_EXCHANGE_ACTION_RETRY, NFC_ROLE_SEND, NfcExchangeEngine,
    WorkflowEngine,
};
use vauchi_core::clock::SystemClock;
use vauchi_core::identity::Identity;

/// Role chooser: `cancel` is the only `ScreenAction` affordance. The send /
/// receive `ActionList` items are navigation edges to the awaiting-tap screen
/// (whose `cancel` is the same id), not affordances.
const ROLE_HANDLED: &[&str] = &[NFC_EXCHANGE_ACTION_CANCEL];
/// Failed (with camera): retry + both fallbacks + cancel.
const FAILED_HANDLED: &[&str] = &[
    NFC_EXCHANGE_ACTION_RETRY,
    "fallback_qr",
    "fallback_relay",
    NFC_EXCHANGE_ACTION_CANCEL,
];

fn identity() -> Identity {
    Identity::create("Alice", SystemClock::shared().unix_seconds())
}

/// Role-chooser root — a fresh engine with an identity renders the chooser.
fn role_factory() -> NfcExchangeEngine {
    NfcExchangeEngine::new(Some(identity()), "Alice".into(), true)
}

/// Failed root — Send with no identity fails gracefully to the failed screen
/// (camera present → the QR fallback is offered).
fn failed_factory() -> NfcExchangeEngine {
    let mut e = NfcExchangeEngine::new(None, "Alice".into(), true);
    let _ = e.handle_action(vauchi_app::ui::UserAction::ListItemSelected {
        component_id: "nfc_role".into(),
        item_id: NFC_ROLE_SEND.into(),
    });
    e
}

// @internal
#[test]
fn role_chooser_is_reachable_with_send_receive_and_cancel() {
    let e = role_factory();
    assert_eq!(e.current_screen().screen_id, "exchange_nfc_role");
    assert_reachability_across_screens(role_factory, ROLE_HANDLED);
}

// @internal
#[test]
fn failed_screen_is_reachable_with_retry_fallbacks_and_cancel() {
    let e = failed_factory();
    assert_eq!(e.current_screen().screen_id, "exchange_failed");
    assert_reachability_across_screens(failed_factory, FAILED_HANDLED);
}

// @internal
#[test]
fn no_orphans_on_role_chooser() {
    let report = check_reachability(role_factory, ROLE_HANDLED);
    assert!(report.is_reachable(), "role chooser orphans: {report:?}");
}

// @internal
#[test]
fn no_orphans_on_failed_screen() {
    let report = check_reachability(failed_factory, FAILED_HANDLED);
    assert!(report.is_reachable(), "failed orphans: {report:?}");
}
