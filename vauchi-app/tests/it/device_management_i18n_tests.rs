// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the device-management
//! screen renders in the user's locale.
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{DeviceListItem, DeviceManagementEngine, WorkflowEngine};

fn sample_devices() -> Vec<DeviceListItem> {
    vec![DeviceListItem {
        device_index: 0,
        device_name: "Pixel 8".into(),
        public_key_prefix: "abcd1234".into(),
        is_current: true,
        is_active: true,
    }]
}

/// `(title, revoke action label)` for the device-management screen.
fn management_copy(locale: Locale) -> (String, String) {
    let engine = DeviceManagementEngine::new(sample_devices()).with_locale(locale);
    let screen = engine.current_screen();
    (screen.title.clone(), action_label(&screen, "revoke_device"))
}

// @scenario: devices :: device-management screen renders in the active locale
// @internal
#[test]
fn device_management_renders_the_active_locale() {
    load_german();
    let (de_title, de_revoke) = management_copy(Locale::German);
    let (en_title, en_revoke) = management_copy(Locale::English);

    assert_translated("device-management title", &de_title, &en_title);
    assert_translated("revoke-device action", &de_revoke, &en_revoke);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn device_management_english_copy_unchanged() {
    let engine = DeviceManagementEngine::new(sample_devices());
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Devices");
    assert_eq!(action_label(&screen, "revoke_device"), "Revoke Device");
}
