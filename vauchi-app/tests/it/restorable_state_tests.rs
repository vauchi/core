// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A language or theme picked in Core's Settings is kept by the shell as an
//! opaque value, outside the vault, and handed back in every render-context
//! push, so the pick applies before storage opens and the same in every
//! unlock mode (vauchi/private#814). The theme store is process-global;
//! nextest runs each test in its own process.

use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    AppEngine, AppScreen, RenderContext, StorageLockPresentation, StorageLockReason, UserAction,
    WorkflowEngine,
};
use vauchi_core::Command;
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::settings::AppearancePick;

const PICKED_THEME: &[u8] = br##"[{
    "id": "picked","name": "Picked","version": "1.0.0","mode": "dark",
    "colors": {
        "bg-primary": "#282a36","bg-secondary": "#21222c","bg-tertiary": "#44475a",
        "text-primary": "#f8f8f2","text-secondary": "#bfbfbf",
        "accent": "#bd93f9","accent-dark": "#9580ff",
        "success": "#50fa7b","error": "#ff5555","warning": "#ffb86c","border": "#6272a4"
    }
}]"##;
const PICKED_BACKGROUND: &str = "#282a36";

/// A device's data directory and the key that opens it, kept across
/// restarts.
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

fn open_vauchi(device: &Device) -> Vauchi {
    let config = VauchiConfig::with_storage_path(device.dir.path().join("vauchi.db"))
        .with_storage_key(device.key.clone());
    let mut vauchi = Vauchi::new(config).unwrap();
    if !vauchi.has_identity() {
        vauchi.create_identity("Alice").unwrap();
    }
    vauchi
}

fn open(device: &Device) -> AppEngine {
    let mut engine = AppEngine::new(open_vauchi(device));
    engine
        .reload_design_assets(Some(PICKED_THEME), None)
        .expect("the test theme loads");
    engine
}

fn pick(engine: &mut AppEngine, dropdown: &str, option: &str) {
    engine.navigate_to(AppScreen::SettingsAppearance);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: dropdown.to_string(),
        item_id: option.to_string(),
    });
}

fn kept_states(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::SaveRestorableState { state } => Some(state.clone()),
            _ => None,
        })
        .collect()
}

/// The value a shell holds after the person picked German and the test
/// theme on some device.
fn state_for_german_and_picked() -> String {
    let device = device();
    let mut engine = open(&device);
    pick(&mut engine, "language", "de");
    pick(&mut engine, "theme", "picked");
    kept_states(&engine.drain_pending_commands())
        .pop()
        .expect("a pick hands the shell a value to keep")
}

fn push(state: Option<String>, legacy_theme: Option<&str>) -> RenderContext {
    RenderContext {
        restorable_state: state,
        theme_id: legacy_theme.map(str::to_string),
        locale: legacy_theme.map(|_| "fr".to_string()),
        ..RenderContext::default()
    }
}

fn store_vault_pick(device: &Device, theme_id: &str) {
    let vauchi = open_vauchi(device);
    let mut flags = vauchi.load_settings_flags().unwrap();
    flags.appearance = Some(AppearancePick {
        locale: None,
        theme_id: Some(theme_id.to_string()),
    });
    vauchi.save_settings_flags(&flags).unwrap();
}

// @internal
#[test]
fn each_pick_hands_the_shell_one_value_to_keep() {
    let device = device();
    let mut engine = open(&device);

    pick(&mut engine, "theme", "picked");

    assert_eq!(kept_states(&engine.drain_pending_commands()).len(), 1);
}

// @internal
#[test]
fn the_kept_value_restores_the_pick_on_a_device_whose_vault_never_saw_it() {
    let state = state_for_german_and_picked();
    let device = device();
    let mut engine = open(&device);

    engine.set_render_context(push(Some(state), None));

    assert_eq!(engine.render_context().locale.as_deref(), Some("de"));
    assert_eq!(engine.render_context().theme_id.as_deref(), Some("picked"));
}

// @internal
#[test]
fn the_kept_value_beats_the_shells_own_stored_choice() {
    let state = state_for_german_and_picked();
    let device = device();
    let mut engine = open(&device);

    engine.set_render_context(push(Some(state), Some("classic")));

    assert_eq!(engine.render_context().theme_id.as_deref(), Some("picked"));
    assert_eq!(engine.render_context().locale.as_deref(), Some("de"));
}

// @internal
#[test]
fn a_damaged_kept_value_is_ignored_and_the_shells_own_choice_stands() {
    let valid = state_for_german_and_picked();
    let damaged = [
        String::new(),
        "not json".to_string(),
        "{}".to_string(),
        "null".to_string(),
        "[]".to_string(),
        valid.replacen("picked", "../../etc/passwd", 1),
        valid.replacen("picked", "PICKED\u{0}", 1),
        valid.replacen("picked", &"a".repeat(65), 1),
        valid.replacen("picked", "pïcked", 1),
        valid.replacen("\"de\"", "\"xx-not-a-language\"", 1),
        valid.replacen('}', ",\"extra\":true}", 1),
        format!("{valid}{}", " ".repeat(2048)),
        valid[..valid.len() - 1].to_string(),
    ];
    for state in damaged {
        let device = device();
        let mut engine = open(&device);

        engine.set_render_context(push(Some(state.clone()), Some("classic")));

        assert_eq!(
            (
                engine.render_context().theme_id.as_deref(),
                engine.render_context().locale.as_deref()
            ),
            (Some("classic"), Some("fr")),
            "a damaged value must leave the shell's own choice: {state:?}"
        );
    }
}

// @internal
#[test]
fn a_pick_kept_in_the_vault_before_is_handed_to_the_shell_to_keep() {
    let device = device();
    store_vault_pick(&device, "picked");
    let mut engine = open(&device);

    engine.set_render_context(push(None, None));

    let state = kept_states(&engine.drain_pending_commands())
        .pop()
        .expect("the vault pick is handed to the shell");
    let mut other = open(&self::device());
    other.set_render_context(push(Some(state), None));
    assert_eq!(other.render_context().theme_id.as_deref(), Some("picked"));
}

// @internal
#[test]
fn once_the_shell_hands_a_value_back_the_vault_copy_is_dropped() {
    let device = device();
    store_vault_pick(&device, "picked");
    let state = state_for_german_and_picked();
    let mut engine = open(&device);
    engine.set_render_context(push(Some(state), None));
    drop(engine);

    let mut engine = open(&device);
    engine.set_render_context(push(None, None));

    assert_eq!(engine.render_context().theme_id, None);
}

// @internal
#[test]
fn a_shell_that_keeps_nothing_still_gets_the_pick_from_the_vault() {
    let device = device();
    let mut engine = open(&device);
    pick(&mut engine, "theme", "picked");
    drop(engine);

    let mut engine = open(&device);
    engine.set_render_context(push(None, None));

    assert_eq!(engine.render_context().theme_id.as_deref(), Some("picked"));
}

fn lock_surface(lock: &mut StorageLockPresentation) -> vauchi_core::SurfaceSpec {
    lock.initial_commands()
        .into_iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface),
            _ => None,
        })
        .expect("the lock screen presents a surface")
}

// @internal
#[test]
fn the_lock_screen_paints_the_kept_pick_in_the_kept_language() {
    let state = state_for_german_and_picked();
    let mut german = StorageLockPresentation::new(StorageLockReason::Locked, Locale::ENGLISH);
    german.set_locale(Locale::from_code("de").expect("German"));
    let german_title = lock_surface(&mut german).title;
    let mut lock = StorageLockPresentation::new(StorageLockReason::Locked, Locale::ENGLISH);

    lock.set_render_context(push(Some(state), None));
    let surface = lock_surface(&mut lock);

    assert_eq!(surface.title, german_title);
    assert_eq!(surface.tokens.colors.background, PICKED_BACKGROUND);
}
