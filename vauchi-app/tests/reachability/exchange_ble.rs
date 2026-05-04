// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for the BLE branch of `ExchangeEngine`.
//!
//! Phase 1 of `_private/docs/problems/2026-05-03-ble-exchange-humble-ui-migration`.
//! The container choice locked in Phase 0 puts the BLE flow inside
//! the existing `ExchangeEngine` (sub-flow into `ExchangeStep::Ble`),
//! not a dedicated engine. This test declares the action_id set for
//! every per-step screen the BLE branch can render and asserts no
//! orphan handlers / no orphan affordances per screen.
//!
//! BFS coverage notes:
//! - Sub-flow advancement is `ExchangeHardwareEvent`-driven, not
//!   user-action-driven. The static-diff harness compares
//!   `ActionPressed` action ids only, so per-step factories advance
//!   the engine via hardware events, then `assert_reachability`
//!   pins the affordance set on the current screen.
//! - `Handshaking` and `Exchanging` share `build_exchanging_screen`
//!   (parent dispatch in `exchange.rs:726` matches both arms).
//!   The screen has zero affordances by design — BLE flow is a
//!   wait-state for the user.
//! - `Verifying` similarly has zero affordances.
//! - `Complete` returns `ScreenModel::default()` then immediately
//!   transitions to `ExchangeStep::Success` via parent — covered
//!   by the success-screen reachability assertion.
//! - The Failed screen with BLE-fallback flag set offers
//!   `retry` / `fallback_relay` / `cancel` (with `fallback_qr`
//!   added when the device has a camera; default `DeviceCapabilities`
//!   has no camera, so the test pins the no-camera path).

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{ExchangeConfig, ExchangeEngine, WorkflowEngine};
use vauchi_core::exchange::ExchangeHardwareEvent;
use vauchi_core::exchange::audio_modem::{AudioConfig, generate_fsk_samples};
use vauchi_core::exchange::mode::ExchangeMode;

// Action sets per BLE-branch screen (matches handler arms in
// `core/vauchi-app/src/ui/exchange.rs` and screen builders in
// `core/vauchi-app/src/ui/exchange_ble.rs`).
const DISCOVERING_HANDLED: &[&str] = &["cancel"];
const HANDSHAKING_HANDLED: &[&str] = &[];
const EXCHANGING_HANDLED: &[&str] = &[];
const VERIFYING_HANDLED: &[&str] = &[];
// `qr_fallback_available` stays false because default `DeviceCapabilities`
// has no camera; with a camera the screen would also offer `fallback_qr`.
const FAILED_BLE_NO_CAMERA_HANDLED: &[&str] = &["retry", "fallback_relay", "cancel"];
const SUCCESS_HANDLED: &[&str] = &["done"];

fn config_for_mode(mode: ExchangeMode) -> ExchangeConfig {
    ExchangeConfig {
        own_name: "Test".into(),
        own_qr_data: "v1:test".into(),
        available_groups: Vec::new(),
        device_capabilities: Default::default(),
        mode: Some(mode),
        card_snapshot: None,
    }
}

fn discovering_factory(mode: ExchangeMode) -> ExchangeEngine {
    ExchangeEngine::new(config_for_mode(mode))
}

fn handshaking_factory(mode: ExchangeMode) -> ExchangeEngine {
    let mut e = discovering_factory(mode);
    e.handle_hardware_event(ExchangeHardwareEvent::BleDeviceDiscovered {
        id: "peer-1".into(),
        rssi: -45,
        adv_data: vec![],
    });
    e
}

fn exchanging_factory(mode: ExchangeMode) -> ExchangeEngine {
    let mut e = handshaking_factory(mode);
    e.handle_hardware_event(ExchangeHardwareEvent::BleConnected {
        device_id: "peer-1".into(),
    });
    e
}

/// Bump-mode Verifying factory — easiest to reach:
/// `ImpactDetected` from Exchanging triggers `try_complete`; if card
/// hasn't arrived yet, the flow advances to `Verifying`.
fn bump_verifying_factory() -> ExchangeEngine {
    let mut e = exchanging_factory(ExchangeMode::Bump);
    // Strong impact → proximity verified, but no card yet → advances to Verifying
    // (per `proximity_done_before_card_advances_to_verifying` test).
    e.handle_hardware_event(ExchangeHardwareEvent::ImpactDetected {
        timestamp_ms: 100,
        magnitude_milli_g: 3500,
    });
    e
}

/// Magic-mode full-flow Success factory — drives the engine through
/// the same path as the `magic_full_flow_discovery_to_success`
/// integration test in `exchange.rs::tests`.
fn magic_success_factory() -> ExchangeEngine {
    let mut e = exchanging_factory(ExchangeMode::Magic);
    // Card arrives
    e.handle_hardware_event(ExchangeHardwareEvent::BleCharacteristicNotified {
        uuid: "card".into(),
        data: vec![1, 2, 3],
    });
    // Audio proximity completes via FSK-encoded sample buffer
    let modem = AudioConfig::default();
    let samples = generate_fsk_samples(&[0xAA], &modem);
    e.handle_hardware_event(ExchangeHardwareEvent::AudioSamplesRecorded {
        samples,
        sample_rate: modem.sample_rate,
    });
    e
}

/// BLE-disconnected from Discovering → Failed with `ble_fallback_available`.
fn ble_disconnect_failed_factory() -> ExchangeEngine {
    let mut e = discovering_factory(ExchangeMode::Magic);
    e.handle_hardware_event(ExchangeHardwareEvent::BleDisconnected {
        reason: "peer hung up".into(),
    });
    e
}

// ── Discovering screen — all 3 modes share the structure ──────────

// @internal
#[test]
fn magic_discovering_screen_offers_cancel() {
    let e = discovering_factory(ExchangeMode::Magic);
    assert_eq!(e.current_screen().screen_id, "exchange_ble_discovering");
    assert_reachability(&e, DISCOVERING_HANDLED);
}

// @internal
#[test]
fn bump_discovering_screen_offers_cancel() {
    let e = discovering_factory(ExchangeMode::Bump);
    assert_eq!(e.current_screen().screen_id, "exchange_ble_discovering");
    assert_reachability(&e, DISCOVERING_HANDLED);
}

// @internal
#[test]
fn shake_discovering_screen_offers_cancel() {
    let e = discovering_factory(ExchangeMode::Shake);
    assert_eq!(e.current_screen().screen_id, "exchange_ble_discovering");
    assert_reachability(&e, DISCOVERING_HANDLED);
}

// ── Handshaking + Exchanging — wait-states with no affordances ────

// @internal
#[test]
fn handshaking_screen_has_no_affordances() {
    let e = handshaking_factory(ExchangeMode::Magic);
    // Handshaking + Exchanging share `build_exchanging_screen` per
    // parent dispatch arm in exchange.rs:726.
    assert_eq!(e.current_screen().screen_id, "exchange_ble_exchanging");
    assert_reachability(&e, HANDSHAKING_HANDLED);
}

// @internal
#[test]
fn exchanging_screen_has_no_affordances() {
    let e = exchanging_factory(ExchangeMode::Magic);
    assert_eq!(e.current_screen().screen_id, "exchange_ble_exchanging");
    assert_reachability(&e, EXCHANGING_HANDLED);
}

// ── Verifying — wait-state with no affordances ────────────────────

// @internal
#[test]
fn verifying_screen_has_no_affordances() {
    let e = bump_verifying_factory();
    assert_eq!(e.current_screen().screen_id, "exchange_ble_verifying");
    assert_reachability(&e, VERIFYING_HANDLED);
}

// ── Failed (BLE flavor) — retry + fallback_relay + cancel ─────────

// @internal
#[test]
fn ble_failed_screen_offers_retry_relay_fallback_and_cancel() {
    let e = ble_disconnect_failed_factory();
    assert_eq!(e.current_screen().screen_id, "exchange_failed");
    assert_reachability(&e, FAILED_BLE_NO_CAMERA_HANDLED);
}

// ── Success (post-BLE-flow) — done ────────────────────────────────

// @internal
#[test]
fn ble_success_screen_offers_done() {
    let e = magic_success_factory();
    assert_eq!(e.current_screen().screen_id, "exchange_success");
    assert_reachability(&e, SUCCESS_HANDLED);
}
