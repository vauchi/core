// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Each exchange mode has one pictogram, shown wherever the mode appears, so
//! a mode is recognisable whatever language its name is in (#473). Shells map
//! `pictogram.<group>.<name>` to the bundled `pictograms/<group>/<name>.svg`.

use serde_json::Value;
use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{AppEngine, AppScreen, ScreenModel, UserAction, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::types::AudioCapability;

const MODES: [&str; 9] = [
    "glance",
    "hover",
    "bump",
    "shake",
    "magic",
    "tap_tap",
    "tap_hover_shake",
    "link",
    "cable",
];

fn pictogram(mode: &str) -> String {
    format!("pictogram.exchange.{mode}")
}

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

fn engine_on_picker(caps: DeviceCapabilities) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(caps);
    engine.navigate_to(AppScreen::Exchange);
    engine
}

fn expand(engine: &mut AppEngine) {
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "more".into(),
        item_id: "show_other_modes".into(),
    });
}

fn objects(value: &Value, out: &mut Vec<Value>) {
    match value {
        Value::Object(map) => {
            out.push(value.clone());
            map.values().for_each(|v| objects(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| objects(v, out)),
        _ => {}
    }
}

fn all_objects(screen: &ScreenModel) -> Vec<Value> {
    let mut out = Vec::new();
    objects(
        &serde_json::to_value(screen).expect("screen serializes"),
        &mut out,
    );
    out
}

/// `(mode, icon)` for every picker row naming a mode, whether it selects the
/// mode (`mode:<m>`) or asks for a permission first (`grant:<m>:<req>`).
fn picker_rows(screen: &ScreenModel) -> Vec<(String, Option<String>)> {
    all_objects(screen)
        .iter()
        .filter_map(|o| {
            let id = o.get("id")?.as_str()?;
            let mode = id
                .strip_prefix("mode:")
                .or_else(|| id.strip_prefix("grant:")?.split(':').next())?;
            let icon = o.get("icon").and_then(Value::as_str).map(str::to_owned);
            Some((mode.to_owned(), icon))
        })
        .collect()
}

// @internal
#[test]
fn every_picker_row_shows_its_modes_pictogram() {
    let mut engine = engine_on_picker(full_caps());
    expand(&mut engine);
    let rows = picker_rows(&engine.current_screen());
    assert!(!rows.is_empty(), "the expanded picker offers modes");
    for (mode, icon) in rows {
        assert_eq!(icon, Some(pictogram(&mode)), "picker row for {mode}");
    }
}

// A row asking for a permission is still that mode; the detail line says
// what to grant.
// @internal
#[test]
fn a_permission_row_keeps_its_modes_pictogram() {
    let mut engine = engine_on_picker(full_caps());
    let _ = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "camera".into(),
    });
    expand(&mut engine);
    let rows = picker_rows(&engine.current_screen());
    assert!(
        rows.iter().any(|(mode, _)| mode == "glance"),
        "Glance is offered as a camera grant row: {rows:?}"
    );
    for (mode, icon) in rows {
        assert_eq!(icon, Some(pictogram(&mode)), "picker row for {mode}");
    }
}

// @internal
#[test]
fn every_mode_screen_opens_with_its_pictogram_and_name() {
    for mode in MODES {
        let mut engine = engine_on_picker(full_caps());
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:quick".into(),
            item_id: format!("mode:{mode}"),
        });
        let screen = engine.current_screen();
        let name = get_string(Locale::English, &format!("exchange.mode_name.{mode}"));
        let badge = all_objects(&screen).into_iter().find(|o| {
            o.get("icon").and_then(Value::as_str) == Some(pictogram(mode).as_str())
                && o.get("title").and_then(Value::as_str) == Some(name.as_str())
        });
        assert!(
            badge.is_some(),
            "{mode}: screen {} shows no {} labelled {name:?}",
            screen.screen_id,
            pictogram(mode)
        );
    }
}
