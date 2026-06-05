// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `BleExchangeEngine` (CC-22).
//!
//! Slice 4 of `2026-05-11-ble-exchange-engine-graduation`. Replaces the
//! retired `exchange_ble.rs` reachability test, which drove the BLE branch of
//! the legacy `ExchangeEngine` (removed in slice 3). The dedicated engine
//! wraps `BleExchangeFlow` and renders five screens — discovering, exchanging
//! (shared by Handshaking + Exchanging), verifying, success, failed. CC-22
//! requires the declared handler set match what the BFS walker emits.
//!
//! BFS coverage notes:
//! - The BLE sub-flow advances on hardware *events*, not user actions, and the
//!   walker only follows `ActionPressed` button presses. So each screen is its
//!   own BFS root reached by a per-state factory (mirrors
//!   `multi_stage_exchange.rs` / `link_exchange.rs`).
//! - Discovering exposes only `cancel` (→ terminal `Complete`).
//! - Handshaking + Exchanging share `build_exchanging_screen` — a wait-state
//!   with zero affordances. Verifying likewise has none.
//! - Success exposes `done`.
//! - Failed (`has_camera = true`) exposes `retry` + `fallback_qr` +
//!   `fallback_relay` + `cancel`. `retry` resets to the Discovering screen
//!   (whose `cancel` is already in the set); the other three terminate.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{
    BLE_EXCHANGE_ACTION_CANCEL, BLE_EXCHANGE_ACTION_DONE, BLE_EXCHANGE_ACTION_RETRY,
    BleExchangeEngine, WorkflowEngine,
};
use vauchi_core::Event;
use vauchi_core::exchange::audio_modem::{AudioConfig, generate_fsk_samples};
use vauchi_core::exchange::mode::ExchangeMode;

/// Discovering exposes only `cancel`.
const DISCOVERING_HANDLED: &[&str] = &[BLE_EXCHANGE_ACTION_CANCEL];
/// Handshaking + Exchanging (shared screen) — wait-state, no affordances.
const EXCHANGING_HANDLED: &[&str] = &[];
/// Verifying — wait-state, no affordances.
const VERIFYING_HANDLED: &[&str] = &[];
/// Success exposes only `done`.
const SUCCESS_HANDLED: &[&str] = &[BLE_EXCHANGE_ACTION_DONE];
/// Failed (with camera) exposes `retry` + `fallback_qr` + `fallback_relay` +
/// `cancel`. `retry` navigates back to Discovering (whose `cancel` is already
/// in this set), so the set is closed under the walker.
const FAILED_HANDLED: &[&str] = &[
    BLE_EXCHANGE_ACTION_RETRY,
    "fallback_qr",
    "fallback_relay",
    BLE_EXCHANGE_ACTION_CANCEL,
];

/// Discovering root — a fresh engine renders the discovering screen.
fn discovering_factory() -> BleExchangeEngine {
    BleExchangeEngine::new(ExchangeMode::Magic, true)
}

/// Exchanging root — `BleDeviceDiscovered` advances Discovering → Handshaking,
/// which renders the shared exchanging screen.
fn exchanging_factory() -> BleExchangeEngine {
    let mut e = BleExchangeEngine::new(ExchangeMode::Magic, true);
    e.handle_hardware_event(Event::BleDeviceDiscovered {
        id: "peer-1".into(),
        rssi: -45,
        adv_data: vec![],
    });
    e
}

/// Verifying root (Bump mode) — discover → connect → strong impact with no card
/// yet advances Exchanging → Verifying (per the flow's
/// `proximity_done_before_card_advances_to_verifying` path).
fn verifying_factory() -> BleExchangeEngine {
    let mut e = BleExchangeEngine::new(ExchangeMode::Bump, true);
    e.handle_hardware_event(Event::BleDeviceDiscovered {
        id: "peer-1".into(),
        rssi: -45,
        adv_data: vec![],
    });
    e.handle_hardware_event(Event::BleConnected {
        device_id: "peer-1".into(),
    });
    e.handle_hardware_event(Event::ImpactDetected {
        timestamp_ms: 100,
        magnitude_milli_g: 3500,
    });
    e
}

/// Success root (Magic mode) — discover → connect → card arrives → audio
/// proximity completes via an FSK-encoded sample buffer.
fn success_factory() -> BleExchangeEngine {
    let mut e = BleExchangeEngine::new(ExchangeMode::Magic, true);
    e.handle_hardware_event(Event::BleDeviceDiscovered {
        id: "peer-1".into(),
        rssi: -45,
        adv_data: vec![],
    });
    e.handle_hardware_event(Event::BleConnected {
        device_id: "peer-1".into(),
    });
    e.handle_hardware_event(Event::BleCharacteristicNotified {
        uuid: "card".into(),
        data: vec![1, 2, 3],
    });
    let modem = AudioConfig::default();
    let samples = generate_fsk_samples(&[0xAA], &modem);
    e.handle_hardware_event(Event::AudioSamplesRecorded {
        samples,
        sample_rate: modem.sample_rate,
    });
    e
}

/// Failed root — `BleDisconnected` from Discovering flips to the failed screen
/// with all fallbacks (camera present).
fn failed_factory() -> BleExchangeEngine {
    let mut e = BleExchangeEngine::new(ExchangeMode::Magic, true);
    e.handle_hardware_event(Event::BleDisconnected {
        reason: "peer hung up".into(),
    });
    e
}

// ── Discovering — cancel only ─────────────────────────────────────

// @internal
#[test]
fn discovering_screen_is_reachable_with_cancel() {
    let e = discovering_factory();
    assert_eq!(e.current_screen().screen_id, "exchange_ble_discovering");
    assert_reachability_across_screens(discovering_factory, DISCOVERING_HANDLED);
}

// ── Handshaking / Exchanging — wait-state, no affordances ──────────

// @internal
#[test]
fn exchanging_screen_is_reachable_with_no_actions() {
    let e = exchanging_factory();
    assert_eq!(e.current_screen().screen_id, "exchange_ble_exchanging");
    assert_reachability_across_screens(exchanging_factory, EXCHANGING_HANDLED);
}

// ── Verifying — wait-state, no affordances ────────────────────────

// @internal
#[test]
fn verifying_screen_is_reachable_with_no_actions() {
    let e = verifying_factory();
    assert_eq!(e.current_screen().screen_id, "exchange_ble_verifying");
    assert_reachability_across_screens(verifying_factory, VERIFYING_HANDLED);
}

// ── Success — done ────────────────────────────────────────────────

// @internal
#[test]
fn success_screen_is_reachable_with_done() {
    let e = success_factory();
    assert_eq!(e.current_screen().screen_id, "exchange_success");
    assert_reachability_across_screens(success_factory, SUCCESS_HANDLED);
}

// ── Failed — retry + fallback_qr + fallback_relay + cancel ─────────

// @internal
#[test]
fn failed_screen_is_reachable_with_retry_fallbacks_and_cancel() {
    let e = failed_factory();
    assert_eq!(e.current_screen().screen_id, "exchange_failed");
    assert_reachability_across_screens(failed_factory, FAILED_HANDLED);
}

// ── No-orphan assertions per screen ───────────────────────────────

// @internal
#[test]
fn no_orphans_on_discovering_screen() {
    let report = check_reachability(discovering_factory, DISCOVERING_HANDLED);
    assert!(
        report.is_reachable(),
        "discovering: unexpected orphans: {report:?}"
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
