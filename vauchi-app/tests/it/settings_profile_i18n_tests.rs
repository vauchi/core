// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the settings profile and
//! privacy groups render in the user's locale. Keys in `settings.*`
//! (locales!94).
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::ui::{Component, SettingsConfig, SettingsEngine, WorkflowEngine};

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

/// `(profile group label, edit-profile item, privacy group label, first theme option)`.
fn settings_copy(language_id: &str) -> (String, String, String, String) {
    let engine = SettingsEngine::new(config(language_id));
    let screen = engine.current_screen();

    let Component::SettingsGroup { label, items, .. } = find_group(&screen.components, "profile")
    else {
        unreachable!()
    };
    let profile_label = label.clone();
    let edit_profile = items[1].label.clone();

    // M6 S1b: privacy merged with notifications.
    let Component::SettingsGroup { label, .. } =
        find_group(&screen.components, "privacy_notifications")
    else {
        unreachable!()
    };
    let privacy_label = label.clone();

    let theme = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::Dropdown { id, .. } if id == "theme"))
        .expect("theme dropdown present");
    let Component::Dropdown { options, .. } = theme else {
        unreachable!()
    };

    (
        profile_label,
        edit_profile,
        privacy_label,
        options[0].label.clone(),
    )
}

// @scenario: settings :: profile/privacy groups render in the active locale
// @internal
#[test]
fn settings_profile_groups_render_the_active_locale() {
    load_german();
    let (de_profile, de_edit, de_privacy, de_theme) = settings_copy("de");
    let (en_profile, en_edit, en_privacy, en_theme) = settings_copy("");

    assert_translated("profile group label", &de_profile, &en_profile);
    assert_translated("edit-profile item", &de_edit, &en_edit);
    assert_translated("privacy group label", &de_privacy, &en_privacy);

    // Exemption: the "System" theme option is deliberately identical in
    // both locales, so it cannot be asserted as translated. Pinning it is
    // the point — it names the OS setting, not a translatable phrase.
    assert_eq!(de_theme, "System");
    assert_eq!(de_theme, en_theme);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
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
