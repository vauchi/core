// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The language and theme picked in Core's Settings survive a restart
//! (vauchi/private#790). Core renders Settings, so Core keeps the pick:
//! an app's start-up push still brings the system language, but no longer
//! replaces a pick made in Settings. Before the first pick, an app's own
//! stored choice still applies.

use vauchi_app::ui::{AppEngine, AppScreen, RenderContext, UserAction, WorkflowEngine};
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;

/// The app's data directory and the key that opens it, as a device keeps
/// both across restarts.
struct Device {
    dir: tempfile::TempDir,
    key: SymmetricKey,
}

fn device() -> Device {
    Device {
        dir: tempfile::tempdir().unwrap(),
        key: SymmetricKey::generate(),
    }
}

fn open(device: &Device) -> AppEngine {
    let config = VauchiConfig::with_storage_path(device.dir.path().join("vauchi.db"))
        .with_storage_key(device.key.clone());
    let mut vauchi = Vauchi::new(config).unwrap();
    if !vauchi.has_identity() {
        vauchi.create_identity("Alice").unwrap();
    }
    AppEngine::new(vauchi)
}

fn pick(engine: &mut AppEngine, dropdown: &str, option: &str) {
    engine.navigate_to(AppScreen::SettingsAppearance);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: dropdown.to_string(),
        item_id: option.to_string(),
    });
}

/// What an app pushes at start: the system language, and its own stored
/// language and theme, which may be stale.
fn app_push(locale: Option<&str>, theme_id: Option<&str>) -> RenderContext {
    RenderContext {
        locale: locale.map(str::to_string),
        system_locale: Some("fr-CH".to_string()),
        theme_id: theme_id.map(str::to_string),
    }
}

// @internal
#[test]
fn a_picked_language_and_theme_survive_reopening() {
    let device = device();
    let mut engine = open(&device);
    pick(&mut engine, "language", "en");
    pick(&mut engine, "theme", "cyber");
    drop(engine);

    let engine = open(&device);
    assert_eq!(engine.render_context().locale.as_deref(), Some("en"));
    assert_eq!(engine.render_context().theme_id.as_deref(), Some("cyber"));
}

// @internal
#[test]
fn an_app_push_after_a_pick_keeps_the_pick_and_brings_the_system_language() {
    let device = device();
    let mut engine = open(&device);
    pick(&mut engine, "language", "en");
    drop(engine);

    let mut engine = open(&device);
    engine.set_render_context(app_push(None, None));
    assert_eq!(engine.render_context().locale.as_deref(), Some("en"));
    assert_eq!(
        engine.render_context().system_locale.as_deref(),
        Some("fr-CH")
    );
}

// @internal
#[test]
fn follow_system_picked_in_settings_beats_a_stale_app_choice() {
    let device = device();
    let mut engine = open(&device);
    pick(&mut engine, "language", "en");
    pick(&mut engine, "language", "follow_system");
    pick(&mut engine, "theme", "follow_system");
    drop(engine);

    let mut engine = open(&device);
    engine.set_render_context(app_push(Some("de"), Some("classic")));
    assert_eq!(engine.render_context().locale, None);
    assert_eq!(engine.render_context().theme_id, None);
}

// @internal
#[test]
fn before_any_pick_the_app_choice_still_applies() {
    let device = device();
    let mut engine = open(&device);
    engine.set_render_context(app_push(Some("de"), Some("classic")));
    assert_eq!(engine.render_context().locale.as_deref(), Some("de"));
    assert_eq!(engine.render_context().theme_id.as_deref(), Some("classic"));
}
