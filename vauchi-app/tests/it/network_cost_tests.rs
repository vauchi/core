// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shells report what the current network costs, so "Wi-Fi only" content
//! updates run on an unmetered network and nowhere else
//! (vauchi/private#733 D6). The report is an observation; Core decides.

use vauchi_app::ui::AppEngine;
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::settings::{ContentUpdates, NetworkCost};

fn engine_with(choice: ContentUpdates) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.config_mut().content_updates = choice;
    AppEngine::new(vauchi)
}

// @internal
#[test]
fn wifi_only_updates_follow_the_reported_network() {
    let mut engine = engine_with(ContentUpdates::WifiOnly);
    assert!(!engine.content_updates_allowed(), "before any report");

    let _ = engine.handle_hardware_event(Event::NetworkCostChanged {
        cost: NetworkCost::Unmetered,
    });
    assert!(engine.content_updates_allowed(), "on an unmetered network");

    let _ = engine.handle_hardware_event(Event::NetworkCostChanged {
        cost: NetworkCost::Metered,
    });
    assert!(!engine.content_updates_allowed(), "on a metered network");
}

// @internal
#[test]
fn the_event_decodes_only_known_network_costs() {
    let unmetered: Event =
        serde_json::from_str(r#"{"NetworkCostChanged":{"cost":"unmetered"}}"#).unwrap();
    assert_eq!(
        unmetered,
        Event::NetworkCostChanged {
            cost: NetworkCost::Unmetered
        }
    );
    for json in [
        r#"{"NetworkCostChanged":{"cost":"cheap"}}"#,
        r#"{"NetworkCostChanged":{"cost":""}}"#,
        r#"{"NetworkCostChanged":{}}"#,
    ] {
        assert!(serde_json::from_str::<Event>(json).is_err(), "{json}");
    }
}
