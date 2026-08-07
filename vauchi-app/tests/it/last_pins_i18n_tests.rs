// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the MyInfo sync captions
//! and the settings About overlay render in the user's locale.
//! Keys: `sync.*` (locales!89) + the existing `about.what_is_vauchi.*`.
//!
//! Asserts that the screens resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`. Interpolation
//! of the pending count is asserted separately, since that IS core's.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    ActionResult, Component, MyInfoEngine, MyInfoProgress, SettingsConfig, SettingsEngine,
    UserAction, WorkflowEngine,
};

fn caption_texts(screen: &vauchi_app::ui::ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::Text { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

/// The MyInfo captions joined, so the whole caption block can be compared.
fn sync_captions(locale: Locale) -> Vec<String> {
    let engine = MyInfoEngine::new(MyInfoProgress::default())
        .with_locale(locale)
        .with_pending_updates(1)
        .with_last_sync_seconds(Some(1_000))
        .with_now_seconds(1_030);
    caption_texts(&engine.current_screen())
}

fn about_overlay_title(language_id: &str) -> String {
    let config = SettingsConfig {
        language_id: language_id.into(),
        ..Default::default()
    };
    let mut engine = SettingsEngine::new(config);
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "help_about".into(),
        item_id: "what_is_vauchi".into(),
    });
    let ActionResult::ShowInfoOverlay { title, .. } = result else {
        panic!("about → ShowInfoOverlay, got {result:?}");
    };
    title
}

// @scenario: identity :: MyInfo sync captions render in the active locale
// @internal
#[test]
fn my_info_sync_captions_render_the_active_locale() {
    load_german();
    let de = sync_captions(Locale::German);
    let en = sync_captions(Locale::English);

    assert_translated("MyInfo caption block", &de.join(" | "), &en.join(" | "));

    // Interpolation IS core's to hold: the pending count must resolve in
    // every locale, whatever the surrounding wording says.
    assert!(
        de.iter().any(|t| t.contains('1')),
        "German pending caption dropped the interpolated count; got {de:?}"
    );
    assert!(
        en.iter().any(|t| t.contains('1')),
        "English pending caption dropped the interpolated count; got {en:?}"
    );
}

// @scenario: settings :: about overlay renders in the active locale
// @internal
#[test]
fn settings_about_overlay_renders_the_active_locale() {
    load_german();
    assert_translated(
        "about overlay title",
        &about_overlay_title("de"),
        &about_overlay_title(""),
    );
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn last_pins_english_copy_unchanged() {
    let engine = MyInfoEngine::new(MyInfoProgress::default())
        .with_pending_updates(3)
        .with_last_sync_seconds(Some(1_000))
        .with_now_seconds(1_030);
    let texts = caption_texts(&engine.current_screen());
    assert!(
        texts.iter().any(|t| t == "3 pending updates"),
        "English pending caption unchanged; got {texts:?}"
    );

    assert_eq!(about_overlay_title(""), "What is Vauchi?");
}
