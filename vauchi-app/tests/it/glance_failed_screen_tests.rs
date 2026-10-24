// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Glance's failed screen offers only ways out that can work (private#669).
//! Glance is the QR mode, so its "switch to QR" fallback restarted Glance and
//! met the same failure, e.g. a refused Bluetooth permission on the s7.

use vauchi_app::ui::{BLE_EXCHANGE_ACTION_RETRY, BleExchangeEngine, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::exchange::mode::ExchangeMode;

fn failed_by(mode: ExchangeMode, event: Event) -> Vec<String> {
    let mut engine = BleExchangeEngine::new(
        mode,
        true,
        vec![],
        vauchi_core::clock::SystemClock::shared(),
        Some("OWN-QR".to_string()),
        vauchi_app::i18n::Locale::ENGLISH,
    );
    engine.handle_hardware_event(event);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_failed");
    screen
        .contextual_actions
        .into_iter()
        .map(|action| action.id)
        .collect()
}

fn ble_permission_denied() -> Event {
    Event::PermissionDenied {
        transport: "ble".into(),
    }
}

// @scenario: exchange :: a failed Glance offers no fallback that restarts Glance
// @internal
#[test]
fn a_failed_glance_does_not_offer_to_switch_to_glance() {
    let actions = failed_by(ExchangeMode::Glance, ble_permission_denied());

    assert!(
        !actions.iter().any(|id| id == "fallback_qr"),
        "switching to QR from Glance restarts Glance: {actions:?}"
    );
    assert!(actions.iter().any(|id| id == BLE_EXCHANGE_ACTION_RETRY));
    assert!(
        actions.iter().any(|id| id == "fallback_relay"),
        "the link needs no Bluetooth: {actions:?}"
    );
}

// @internal
#[test]
fn another_mode_still_falls_back_to_glance() {
    let actions = failed_by(ExchangeMode::Magic, ble_permission_denied());

    assert!(actions.iter().any(|id| id == "fallback_qr"), "{actions:?}");
}
