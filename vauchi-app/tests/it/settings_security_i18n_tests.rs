// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the settings security and
//! danger groups render in the user's locale.
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`. Placeholder
//! interpolation is asserted separately, since that IS core's to hold.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::ui::{Component, SettingsConfig, SettingsEngine, WorkflowEngine};

fn config(language_id: &str) -> SettingsConfig {
    SettingsConfig {
        language_id: language_id.into(),
        device_count: 2,
        ..Default::default()
    }
}

fn find_group<'a>(components: &'a [Component], id: &str) -> &'a Component {
    components
        .iter()
        .find(|c| matches!(c, Component::SettingsGroup { id: gid, .. } if gid == id))
        .unwrap_or_else(|| panic!("group {id} not found; components: {components:?}"))
}

/// `(screen title, security group label, devices detail, danger group label)`.
fn security_copy(language_id: &str) -> (String, String, String, String) {
    let engine = SettingsEngine::new(config(language_id));
    let screen = engine.current_screen();

    // M6 S1b: security merged with backup.
    let Component::SettingsGroup { label, items, .. } =
        find_group(&screen.components, "security_backup")
    else {
        unreachable!()
    };
    let security_label = label.clone();
    let devices_item = items.iter().find(|i| i.id == "devices").unwrap();
    let devices_detail = match &devices_item.kind {
        vauchi_app::ui::SettingsItemKind::Link { detail } => {
            detail.clone().expect("devices row carries a detail")
        }
        other => panic!("devices row is a Link, got {other:?}"),
    };

    // M6 D6.1: danger lives on the Advanced sub-screen now.
    let advanced = SettingsEngine::new_advanced(config(language_id)).current_screen();
    let Component::SettingsGroup { label, .. } = find_group(&advanced.components, "danger") else {
        unreachable!()
    };

    (
        screen.title.clone(),
        security_label,
        devices_detail,
        label.clone(),
    )
}

// @scenario: settings :: security/danger groups render in the active locale
// @internal
#[test]
fn settings_security_groups_render_the_active_locale() {
    load_german();
    let (de_title, de_security, de_devices, de_danger) = security_copy("de");
    let (en_title, en_security, en_devices, en_danger) = security_copy("");

    assert_translated("settings title", &de_title, &en_title);
    assert_translated("security group label", &de_security, &en_security);
    assert_translated("danger group label", &de_danger, &en_danger);
    assert_translated("devices detail", &de_devices, &en_devices);

    // Interpolation IS core's to hold: the device count must resolve in
    // every locale, whatever the surrounding wording says.
    assert!(
        de_devices.contains('2'),
        "German devices detail did not interpolate the count, got {de_devices:?}"
    );
    assert!(
        en_devices.contains('2'),
        "English devices detail did not interpolate the count, got {en_devices:?}"
    );
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn settings_security_groups_english_copy_unchanged() {
    let engine = SettingsEngine::new(config(""));
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Settings");

    let Component::SettingsGroup { label, items, .. } =
        find_group(&screen.components, "security_backup")
    else {
        unreachable!()
    };
    assert_eq!(label, "Security & Backup");
    let devices_item = items.iter().find(|i| i.id == "devices").unwrap();
    assert_eq!(
        devices_item.kind,
        vauchi_app::ui::SettingsItemKind::Link {
            detail: Some("2 devices".into())
        }
    );
}
