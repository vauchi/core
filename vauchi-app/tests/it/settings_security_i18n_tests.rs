// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S6b-5b (`2026-07-03-core-screens-bypass-i18n`): the settings
//! screen's accessibility/security/backup/network/delivery/help/
//! about/danger groups render in the user's locale (derived from
//! `config.language_id`). Keys in `settings.*` (locales!95). Exact
//! German assertions per CC-03.

use vauchi_app::i18n::load_locale_from_bytes;
use vauchi_app::ui::{Component, SettingsConfig, SettingsEngine, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

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

// @scenario: settings :: security/danger groups render in the active locale
// @internal
#[test]
fn settings_security_groups_render_german() {
    load_german();
    let engine = SettingsEngine::new(config("de"));
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Einstellungen");

    let Component::SettingsGroup { label, items, .. } = find_group(&screen.components, "security")
    else {
        unreachable!()
    };
    assert_eq!(label, "Sicherheit");
    let devices_item = items.iter().find(|i| i.id == "devices").unwrap();
    assert_eq!(
        devices_item.kind,
        vauchi_app::ui::SettingsItemKind::Link {
            detail: Some("2 Geräte".into())
        }
    );

    let Component::SettingsGroup { label, .. } = find_group(&screen.components, "danger") else {
        unreachable!()
    };
    assert_eq!(label, "Gefahrenzone");
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn settings_security_groups_english_copy_unchanged() {
    let engine = SettingsEngine::new(config(""));
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Settings");

    let Component::SettingsGroup { label, items, .. } = find_group(&screen.components, "security")
    else {
        unreachable!()
    };
    assert_eq!(label, "Security");
    let devices_item = items.iter().find(|i| i.id == "devices").unwrap();
    assert_eq!(
        devices_item.kind,
        vauchi_app::ui::SettingsItemKind::Link {
            detail: Some("2 devices".into())
        }
    );
}
