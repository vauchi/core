// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `DirectTransportEngine` (CC-22).
//!
//! Cable graduation (`2026-05-11-direct-transport-engine-graduation`). The
//! engine owns a `new_usb` `ExchangeSession`; its exchange / verifying / success
//! screens are driven by USB *hardware* events (DirectPayloadReceived →
//! DirectCardReceived), not actions, so the action walker only reaches the two
//! action-rooted screens:
//!
//! - **Waiting** (`exchange_direct_waiting`) — the entry screen, whose only
//!   affordance is `cancel`.
//! - **Failed** (`exchange_failed`) — reached by feeding an invalid payload;
//!   exposes `retry` (→ `StartDirectTransport`, a navigation edge) + `cancel`.
//!
//! The exchanging / verifying screens (hardware-driven, `cancel`-only or no
//! affordance) and Success (`done`, identical to the shared BLE/Link success
//! screen already covered) are unreachable by the action walker.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{
    DIRECT_TRANSPORT_ACTION_CANCEL, DIRECT_TRANSPORT_ACTION_RETRY, DirectTransportEngine,
    WorkflowEngine,
};
use vauchi_core::clock::SystemClock;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::UsbRole;
use vauchi_core::identity::Identity;

/// Waiting root: `cancel` is the only affordance.
const WAITING_HANDLED: &[&str] = &[DIRECT_TRANSPORT_ACTION_CANCEL];
/// Failed: retry (→ StartDirectTransport navigation) + cancel.
const FAILED_HANDLED: &[&str] = &[
    DIRECT_TRANSPORT_ACTION_RETRY,
    DIRECT_TRANSPORT_ACTION_CANCEL,
];

fn identity() -> Identity {
    Identity::create("Alice", SystemClock::shared().unix_seconds())
}

/// Waiting root — a fresh engine renders the USB-connect waiting screen.
fn waiting_factory() -> DirectTransportEngine {
    let id = identity();
    let card = ContactCard::new(id.display_name());
    DirectTransportEngine::new(
        Some(id),
        Some(card),
        UsbRole::Initiator,
        SystemClock::shared(),
        vauchi_app::i18n::Locale::English,
    )
}

/// Failed root — an invalid payload drives the engine to the failed screen.
fn failed_factory() -> DirectTransportEngine {
    let mut e = waiting_factory();
    let _ = e.handle_hardware_event(vauchi_core::Event::DirectPayloadReceived {
        data: b"not-a-valid-qr".to_vec(),
    });
    e
}

// @internal
#[test]
fn waiting_screen_is_reachable_with_cancel() {
    let e = waiting_factory();
    assert_eq!(e.current_screen().screen_id, "exchange_direct_waiting");
    assert_reachability_across_screens(waiting_factory, WAITING_HANDLED);
}

// @internal
#[test]
fn failed_screen_is_reachable_with_retry_and_cancel() {
    let e = failed_factory();
    assert_eq!(e.current_screen().screen_id, "exchange_failed");
    assert_reachability_across_screens(failed_factory, FAILED_HANDLED);
}

// @internal
#[test]
fn no_orphans_on_waiting() {
    let report = check_reachability(waiting_factory, WAITING_HANDLED);
    assert!(report.is_reachable(), "waiting orphans: {report:?}");
}

// @internal
#[test]
fn no_orphans_on_failed() {
    let report = check_reachability(failed_factory, FAILED_HANDLED);
    assert!(report.is_reachable(), "failed orphans: {report:?}");
}
