// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S6a (`2026-07-03-core-screens-bypass-i18n`): the last two production
//! `Locale::English` pins — the MyInfo home sync captions and the Settings
//! about-overlay — render in the user's locale. MyInfo takes a locale via
//! `with_locale`; Settings resolves it from `SettingsConfig.language_id`.
//! Keys: `sync.*` (locales!89) + the existing `about.what_is_vauchi.*`.
//! Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{
    ActionResult, Component, MyInfoEngine, MyInfoProgress, SettingsConfig, SettingsEngine,
    UserAction, WorkflowEngine,
};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

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

// @scenario: identity :: MyInfo sync captions render in the active locale
// @internal
#[test]
fn my_info_sync_captions_render_german() {
    load_german();
    let engine = MyInfoEngine::new(MyInfoProgress::default())
        .with_locale(Locale::German)
        .with_pending_updates(1)
        .with_last_sync_seconds(Some(1_000))
        .with_now_seconds(1_030);
    let texts = caption_texts(&engine.current_screen());
    assert!(
        texts.iter().any(|t| t == "1 ausstehende Aktualisierung"),
        "pending-updates caption localized; got {texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("Zuletzt synchronisiert")),
        "last-synced caption localized; got {texts:?}"
    );
}

// @scenario: settings :: about overlay renders in the active locale
// @internal
#[test]
fn settings_about_overlay_renders_german() {
    load_german();
    let config = SettingsConfig {
        language_id: "de".into(),
        ..Default::default()
    };
    let mut engine = SettingsEngine::new(config);
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "about".into(),
        item_id: "what_is_vauchi".into(),
    });
    let ActionResult::ShowInfoOverlay { title, .. } = result else {
        panic!("about → ShowInfoOverlay, got {result:?}");
    };
    assert_eq!(title, "Was ist Vauchi?");
}

// English stays exactly as before (regression pin).
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

    let mut engine = SettingsEngine::new(SettingsConfig::default());
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "about".into(),
        item_id: "what_is_vauchi".into(),
    });
    let ActionResult::ShowInfoOverlay { title, .. } = result else {
        panic!("about → ShowInfoOverlay, got {result:?}");
    };
    assert_eq!(title, "What is Vauchi?");
}
