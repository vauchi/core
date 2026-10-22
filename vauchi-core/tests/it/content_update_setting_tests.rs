// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Content updates are a setting: On, Wi-Fi only, Off (vauchi/private#733
//! D6). Wi-Fi only means an unmetered network; an unknown network counts
//! as metered, so an update never runs on a connection the person pays for
//! by mistake.

use vauchi_core::settings::{ContentUpdates, NetworkCost, SettingsFlags};

// @internal
#[test]
fn content_updates_are_on_unless_chosen_otherwise() {
    assert_eq!(ContentUpdates::default(), ContentUpdates::On);
    assert_eq!(SettingsFlags::default().content_updates, ContentUpdates::On);

    let stored_before_the_setting_existed: SettingsFlags = serde_json::from_str("{}").unwrap();
    assert_eq!(
        stored_before_the_setting_existed.content_updates,
        ContentUpdates::On
    );
}

// @internal
#[test]
fn the_setting_decides_on_which_networks_an_update_runs() {
    use ContentUpdates::*;
    use NetworkCost::*;
    for (setting, network, runs) in [
        (On, Unmetered, true),
        (On, Metered, true),
        (On, Unknown, true),
        (WifiOnly, Unmetered, true),
        (WifiOnly, Metered, false),
        (WifiOnly, Unknown, false),
        (Off, Unmetered, false),
        (Off, Metered, false),
        (Off, Unknown, false),
    ] {
        assert_eq!(setting.allows(network), runs, "{setting:?} on {network:?}");
    }
}

// @internal
#[test]
fn each_choice_has_a_stable_id_and_an_unknown_id_is_none() {
    for setting in [
        ContentUpdates::On,
        ContentUpdates::WifiOnly,
        ContentUpdates::Off,
    ] {
        assert_eq!(ContentUpdates::from_id(setting.id()), Some(setting));
    }
    assert_eq!(ContentUpdates::WifiOnly.id(), "wifi_only");
    for id in ["", "ON", "wifi", "off ", "always"] {
        assert_eq!(ContentUpdates::from_id(id), None, "{id:?}");
    }
}

// @internal
#[test]
fn the_choice_survives_a_round_trip_through_stored_flags() {
    let mut flags = SettingsFlags::default();
    flags.content_updates = ContentUpdates::WifiOnly;

    let stored = serde_json::to_string(&flags).unwrap();

    assert_eq!(
        serde_json::from_str::<SettingsFlags>(&stored)
            .unwrap()
            .content_updates,
        ContentUpdates::WifiOnly
    );
}

/// Updates while the app is in the background are a choice of their own
/// (owner, 2026-10-22), on unless switched off, also for flags stored
/// before the choice existed.
// @internal
#[test]
fn background_updates_are_on_unless_switched_off() {
    assert!(SettingsFlags::default().content_updates_in_background);
    let stored_before_the_setting_existed: SettingsFlags = serde_json::from_str("{}").unwrap();
    assert!(stored_before_the_setting_existed.content_updates_in_background);

    let stored_off: SettingsFlags =
        serde_json::from_str(r#"{"content_updates_in_background":false}"#).unwrap();
    assert!(!stored_off.content_updates_in_background);
}
