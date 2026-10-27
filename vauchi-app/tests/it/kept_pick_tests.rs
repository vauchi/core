// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Core keeps the language and theme pick in the data directory it is
//! opened with, outside the vault, so no shell needs storage or start-up
//! wiring of its own for it (vauchi/private#814, #267). The theme store is
//! process-global; nextest runs each test in its own process.

use std::path::Path;

use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    AppEngine, AppScreen, RenderContext, StorageLockPresentation, StorageLockReason, UserAction,
    WorkflowEngine,
};
use vauchi_core::Command;
use vauchi_core::api::Vauchi;

const PICKED_THEME: &[u8] = br##"[{
    "id": "picked","name": "Picked","version": "1.0.0","mode": "dark",
    "colors": {
        "bg-primary": "#282a36","bg-secondary": "#21222c","bg-tertiary": "#44475a",
        "text-primary": "#f8f8f2","text-secondary": "#bfbfbf",
        "accent": "#bd93f9","accent-dark": "#9580ff",
        "success": "#50fa7b","error": "#ff5555","warning": "#ffb86c","border": "#6272a4"
    }
}]"##;

fn engine_keeping_in(dir: &Path) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory core");
    vauchi.create_identity("Alice").expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine
        .reload_design_assets(Some(PICKED_THEME), None)
        .expect("the test theme loads");
    engine.keep_restorable_state_in(dir);
    engine
}

fn pick(engine: &mut AppEngine, dropdown: &str, option: &str) -> Vec<Command> {
    engine.navigate_to(AppScreen::SettingsAppearance);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: dropdown.to_string(),
        item_id: option.to_string(),
    });
    engine.drain_pending_commands()
}

fn picks(engine: &AppEngine) -> (Option<String>, Option<String>) {
    let ctx = engine.render_context();
    (ctx.locale.clone(), ctx.theme_id.clone())
}

// @internal
#[test]
fn a_pick_is_kept_in_the_data_directory_and_applies_at_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = engine_keeping_in(dir.path());
    let _ = pick(&mut first, "language", "de");
    let _ = pick(&mut first, "theme", "picked");
    drop(first);

    let next_start = engine_keeping_in(dir.path());

    assert_eq!(
        picks(&next_start),
        (Some("de".to_string()), Some("picked".to_string()))
    );
}

// @internal
#[test]
fn core_keeping_the_pick_asks_the_shell_to_keep_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine_keeping_in(dir.path());

    let commands = pick(&mut engine, "theme", "picked");

    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, Command::SaveRestorableState { .. })),
        "{commands:?}"
    );
}

// @internal
#[test]
fn a_push_without_a_kept_value_keeps_the_pick() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = engine_keeping_in(dir.path());
    let _ = pick(&mut first, "language", "de");
    drop(first);
    let mut next_start = engine_keeping_in(dir.path());

    next_start.set_render_context(RenderContext {
        system_locale: Some("fr_CH.UTF-8".to_string()),
        ..RenderContext::default()
    });

    assert_eq!(next_start.render_context().locale.as_deref(), Some("de"));
}

// @internal
#[test]
fn a_damaged_or_oversized_kept_file_is_ignored() {
    for content in [b"not json".to_vec(), vec![b' '; 8192], vec![0xff, 0xfe]] {
        let dir = tempfile::tempdir().unwrap();
        let mut first = engine_keeping_in(dir.path());
        let _ = pick(&mut first, "language", "de");
        drop(first);
        let kept: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(kept.len(), 1, "the pick left exactly one file to damage");
        std::fs::write(&kept[0], &content).unwrap();

        let next_start = engine_keeping_in(dir.path());

        assert_eq!(picks(&next_start), (None, None), "{content:?}");
    }
}

// Only the theme is checked: tests carry the English catalogue alone, so a
// German lock screen renders in English (vauchi/private#837).
// @internal
#[test]
fn the_lock_screen_shows_the_kept_pick() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = engine_keeping_in(dir.path());
    let _ = pick(&mut first, "theme", "picked");
    drop(first);
    let mut lock = StorageLockPresentation::new(StorageLockReason::Locked, Locale::ENGLISH);

    lock.keep_restorable_state_in(dir.path());

    assert_eq!(lock_surface(&mut lock).tokens.colors.background, "#282a36");
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
