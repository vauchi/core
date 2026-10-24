// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::{AppEngine, AppScreen, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::identity::Identity;
use vauchi_core::platform::{BleAdvertisementObservation, BleServiceUuid, Command, Event};

fn engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi
        .set_identity(Identity::from_device_link(
            [0; 32],
            "Alice".into(),
            0,
            "test".into(),
            1,
        ))
        .unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);
    engine.select_exchange_mode_for_testing(ExchangeMode::Magic);
    engine
}

fn observed(include_service: bool) -> Event {
    let mut service_uuids = vec![[
        0, 0, 0xff, 0xfe, 0, 0, 0x10, 0, 0x80, 0, 0, 0x80, 0x5f, 0x9b, 0x34, 0xfb,
    ]];
    if include_service {
        service_uuids.push([
            0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56,
            0x78, 0x90,
        ]);
    }
    Event::BleAdvertisementObserved(
        BleAdvertisementObservation::new(
            "peer".into(),
            -42,
            service_uuids
                .into_iter()
                .map(BleServiceUuid::Normalized)
                .collect(),
        )
        .unwrap(),
    )
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[test]
fn canonical_dispatch_decodes_raw_advertisements_before_reducing_exchange() {
    let mut engine = engine();
    let commands = engine.dispatch(observed(true)).unwrap();
    let peers: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::BleConnect { device_id } => Some(device_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(peers, vec!["peer"]);
    assert_eq!(engine.current_screen().screen_id, "exchange_ble_exchanging");
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn unrelated_radio_observations_do_not_start_an_exchange() {
    let mut engine = engine();
    assert_eq!(engine.dispatch(observed(false)).unwrap(), vec![]);
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_ble_discovering"
    );
}
