// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S6b-4 (`2026-07-03-core-screens-bypass-i18n`): the device-
//! management screen (title, per-device detail/hints, revoke
//! confirmation) renders in the user's locale. Keys in `devices.*`
//! (locales!93). Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{DeviceListItem, DeviceManagementEngine, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

fn sample_devices() -> Vec<DeviceListItem> {
    vec![DeviceListItem {
        device_index: 0,
        device_name: "Pixel 8".into(),
        public_key_prefix: "abcd1234".into(),
        is_current: true,
        is_active: true,
    }]
}

// @scenario: devices :: device-management screen renders in the active locale
// @internal
#[test]
fn device_management_renders_german() {
    load_german();
    let engine = DeviceManagementEngine::new(sample_devices()).with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Geräte");
    assert_eq!(
        screen
            .contextual_actions
            .iter()
            .find(|a| a.id == "revoke_device")
            .unwrap()
            .label,
        "Gerät widerrufen"
    );
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn device_management_english_copy_unchanged() {
    let engine = DeviceManagementEngine::new(sample_devices());
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Devices");
    assert_eq!(
        screen
            .contextual_actions
            .iter()
            .find(|a| a.id == "revoke_device")
            .unwrap()
            .label,
        "Revoke Device"
    );
}
