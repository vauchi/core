// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S6b-5a (`2026-07-03-core-screens-bypass-i18n`): the settings
//! screen's profile/privacy/notifications/appearance/theme/language
//! groups render in the user's locale (derived from
//! `config.language_id`, ADR-047 absence-is-follow-system). Keys in
//! `settings.*` (locales!94). Exact German assertions per CC-03.

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
        ..Default::default()
    }
}

fn find_group<'a>(components: &'a [Component], id: &str) -> &'a Component {
    components
        .iter()
        .find(|c| matches!(c, Component::SettingsGroup { id: gid, .. } if gid == id))
        .unwrap_or_else(|| panic!("group {id} not found; components: {components:?}"))
}

// @scenario: settings :: profile/privacy groups render in the active locale
// @internal
#[test]
fn settings_profile_groups_render_german() {
    load_german();
    let engine = SettingsEngine::new(config("de"));
    let screen = engine.current_screen();

    let Component::SettingsGroup { label, items, .. } = find_group(&screen.components, "profile")
    else {
        unreachable!()
    };
    assert_eq!(label, "Profil");
    assert_eq!(items[1].label, "Profil bearbeiten");

    // M6 S1b: privacy merged with notifications.
    let Component::SettingsGroup { label, .. } =
        find_group(&screen.components, "privacy_notifications")
    else {
        unreachable!()
    };
    assert_eq!(label, "Datenschutz & Benachrichtigungen");

    let theme = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::Dropdown { id, .. } if id == "theme"))
        .expect("theme dropdown present");
    let Component::Dropdown { options, .. } = theme else {
        unreachable!()
    };
    assert_eq!(options[0].label, "System");
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn settings_profile_groups_english_copy_unchanged() {
    let engine = SettingsEngine::new(config(""));
    let screen = engine.current_screen();

    let Component::SettingsGroup { label, items, .. } = find_group(&screen.components, "profile")
    else {
        unreachable!()
    };
    assert_eq!(label, "Profile");
    assert_eq!(items[1].label, "Edit Profile");
}
