// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M2 S3 — mode picker: one hero action + "Other ways to connect"
//! disclosure (design D2.3, user-approved 2026-07-04: hero = last-used,
//! first-run hero = Glance; all modes stay reachable; Bump/Shake/Magic
//! carry a visible "Unauthenticated" annotation until their BLE auth
//! tiers land). `2026-07-03-one-tap-exchange` goal 3.

use vauchi_app::ui::{AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::types::{AudioCapability, ExchangeDefaults};

/// Every transport present, so availability never skews the picker shape.
fn full_caps() -> DeviceCapabilities {
    DeviceCapabilities {
        has_nfc: true,
        has_ble: true,
        has_camera: true,
        audio: AudioCapability::Full,
        has_accelerometer: true,
        has_internet: true,
        has_usb_port: true,
        ..Default::default()
    }
}

/// AppEngine on the picker: optional stored defaults, injected caps.
fn engine_on_picker(defaults: Option<ExchangeDefaults>, caps: DeviceCapabilities) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
    if let Some(d) = &defaults {
        vauchi
            .storage()
            .ux()
            .save_exchange_defaults(d)
            .expect("seed defaults");
    }
    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(caps);
    engine.navigate_to(AppScreen::Exchange);
    engine
}

/// Item ids of the `ActionList` with the given component id, if present.
fn list_ids(engine: &AppEngine, list_id: &str) -> Option<Vec<String>> {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            Component::ActionList { id, items } if id == list_id => {
                Some(items.iter().map(|i| i.id.clone()).collect())
            }
            _ => None,
        })
}

fn expand(engine: &mut AppEngine) {
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "more".into(),
        item_id: "show_other_modes".into(),
    });
}

// Repeat user: the hero is the last-used mode; everything else sits
// behind one collapsed disclosure.
// @scenario: exchange :: picker hero is the last-used mode
// @internal
#[test]
fn repeat_hero_is_last_used_mode() {
    let engine = engine_on_picker(
        Some(ExchangeDefaults {
            group_ids: vec![],
            mode: ExchangeMode::Hover,
        }),
        full_caps(),
    );
    assert_eq!(
        list_ids(&engine, "hero").expect("hero list present"),
        vec!["mode:hover".to_string()],
        "hero is exactly the last-used mode"
    );
    assert!(
        list_ids(&engine, "other_modes").is_none(),
        "other modes stay behind the collapsed disclosure"
    );
    assert_eq!(
        list_ids(&engine, "more").expect("disclosure entry present"),
        vec!["show_other_modes".to_string()],
        "one disclosure entry opens the rest"
    );
}

// First run (no stored defaults): the hero is Glance (implemented +
// peer-authenticated — the approved first-run pick).
// @scenario: exchange :: first-run picker hero is Glance
// @internal
#[test]
fn first_run_hero_is_glance() {
    let engine = engine_on_picker(None, full_caps());
    assert_eq!(
        list_ids(&engine, "hero").expect("hero list present"),
        vec!["mode:glance".to_string()],
        "first-run hero is Glance"
    );
}

// Expanding shows every other mode, in the approved order, hero excluded.
// @scenario: exchange :: disclosure lists all other modes in order
// @internal
#[test]
fn disclosure_lists_all_other_modes_in_order() {
    let mut engine = engine_on_picker(
        Some(ExchangeDefaults {
            group_ids: vec![],
            mode: ExchangeMode::Hover,
        }),
        full_caps(),
    );
    expand(&mut engine);
    assert_eq!(
        list_ids(&engine, "other_modes").expect("expanded list present"),
        vec![
            "mode:glance".to_string(),
            "mode:tap_hover_shake".to_string(),
            "mode:link".to_string(),
            "mode:cable".to_string(),
            "mode:bump".to_string(),
            "mode:shake".to_string(),
            "mode:magic".to_string(),
            "mode:tap_tap".to_string(),
        ],
        "approved order, hero (Hover) excluded, all modes reachable"
    );
}

// Bump/Shake/Magic run unauthenticated BLE today — their rows say so.
// @scenario: exchange :: unauthenticated BLE modes are annotated
// @internal
#[test]
fn unauthenticated_ble_modes_are_annotated() {
    let mut engine = engine_on_picker(None, full_caps());
    expand(&mut engine);
    let screen = engine.current_screen();
    let details: Vec<(String, String)> = screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::ActionList { id, items } if id == "other_modes" => Some(
                items
                    .iter()
                    .map(|i| (i.id.clone(), i.detail.clone().unwrap_or_default()))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect();
    for mode in ["mode:bump", "mode:shake", "mode:magic"] {
        let (_, detail) = details
            .iter()
            .find(|(id, _)| id == mode)
            .unwrap_or_else(|| panic!("{mode} present in the expanded list"));
        assert!(
            detail.starts_with("Unauthenticated"),
            "{mode} must carry the Unauthenticated annotation, got `{detail}`"
        );
    }
}

// A stored mode that can't run on this device falls back to Glance.
// @scenario: exchange :: unrunnable last-used mode falls back to Glance
// @internal
#[test]
fn unrunnable_last_used_falls_back_to_glance() {
    let engine = engine_on_picker(
        Some(ExchangeDefaults {
            group_ids: vec![],
            // Hover needs microphone + speaker; give the device neither.
            mode: ExchangeMode::Hover,
        }),
        DeviceCapabilities {
            has_ble: true,
            has_camera: true,
            ..Default::default()
        },
    );
    assert_eq!(
        list_ids(&engine, "hero").expect("hero list present"),
        vec!["mode:glance".to_string()],
        "unrunnable last-used mode must not be the hero; Glance is"
    );
}
