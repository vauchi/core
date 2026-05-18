// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! S3 of the settings-storage-by-sensitivity plan
//! (`_private/docs/planning/todo/2026-05-16-settings-storage-by-sensitivity-plan.md`):
//! the Settings ScreenModel's `theme` and `language` Dropdown
//! `selected` values must derive from `engine.render_context()`
//! when the frontend has pushed a value, and fall back to the
//! legacy vault-backed `vauchi.app_preferences()` only during the
//! migration window (until S6).
//!
//! These tests pin the read path. The dual-write intercept
//! contract lives in `settings_render_context_intercept_tests.rs`
//! (S3 task 2).

use vauchi_app::ui::{AppEngine, AppScreen, Component, RenderContext, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Test User").unwrap();
    AppEngine::new(vauchi)
}

fn dropdown_selected(engine: &mut AppEngine, dropdown_id: &str) -> Option<String> {
    engine.navigate_to(AppScreen::Settings);
    let screen = engine.current_screen();
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::Dropdown { id, selected, .. } if id == dropdown_id => Some(selected.clone()),
            _ => None,
        })
        .expect("Settings screen must contain the requested Dropdown")
}

// @internal
#[test]
fn theme_dropdown_selected_reflects_pushed_render_context() {
    let mut engine = engine_with_identity();

    // No RenderContext push yet → falls back to default ("follow_system").
    assert_eq!(
        dropdown_selected(&mut engine, "theme").as_deref(),
        Some("follow_system"),
        "without a RenderContext push the dropdown must fall back to the \
         legacy follow_system default during the migration window"
    );

    // Frontend pushes an explicit theme.
    engine.set_render_context(RenderContext {
        locale: None,
        theme_id: Some("cyber".to_string()),
    });

    assert_eq!(
        dropdown_selected(&mut engine, "theme").as_deref(),
        Some("cyber"),
        "after RenderContext is set with theme_id=Some(\"cyber\"), \
         the Settings theme dropdown's `selected` must reflect the pushed \
         value (S3: read path switches from vault to RenderContext)"
    );
}

// @internal
#[test]
fn language_dropdown_selected_reflects_pushed_render_context() {
    let mut engine = engine_with_identity();

    assert_eq!(
        dropdown_selected(&mut engine, "language").as_deref(),
        Some("follow_system"),
        "without a RenderContext push the dropdown must fall back to the \
         legacy follow_system default during the migration window"
    );

    engine.set_render_context(RenderContext {
        locale: Some("de".to_string()),
        theme_id: None,
    });

    assert_eq!(
        dropdown_selected(&mut engine, "language").as_deref(),
        Some("de"),
        "after RenderContext is set with locale=Some(\"de\"), the Settings \
         language dropdown's `selected` must reflect the pushed value \
         (S3: read path switches from vault to RenderContext)"
    );
}

// @internal
#[test]
fn render_context_fields_are_independent_at_render_time() {
    let mut engine = engine_with_identity();
    engine.set_render_context(RenderContext {
        locale: Some("fr".to_string()),
        theme_id: None,
    });

    // language reflects pushed value
    assert_eq!(
        dropdown_selected(&mut engine, "language").as_deref(),
        Some("fr"),
    );
    // theme is None in RenderContext → falls back to vault default ("follow_system")
    assert_eq!(
        dropdown_selected(&mut engine, "theme").as_deref(),
        Some("follow_system"),
        "theme_id None in RenderContext must fall back to vault during S3 \
         migration window — only fields the frontend has explicitly pushed \
         leave the legacy read path"
    );
}

// @internal
#[test]
fn render_context_overrides_legacy_vault_value() {
    // Migration-correctness gate: even when the vault carries a
    // stale Category-1 value from a pre-S4 install, the
    // frontend-pushed RenderContext is authoritative for what the
    // user sees. (S4 will migrate the vault row to OS-native; S6
    // deletes it. Until then, the read order must prefer the
    // pushed value.)
    use vauchi_core::types::AppPreferences;
    let mut engine = engine_with_identity();
    let vauchi = engine.vauchi();
    let stale = AppPreferences {
        theme_id: Some("classic".to_string()),
        language_code: Some("en".to_string()),
        follow_system_theme: false,
        follow_system_language: false,
    };
    vauchi.set_app_preferences(&stale).unwrap();

    // Without RenderContext push → legacy vault path wins (still S3 behaviour).
    assert_eq!(
        dropdown_selected(&mut engine, "theme").as_deref(),
        Some("classic"),
    );

    // Push RenderContext → RenderContext wins, vault becomes ignored.
    engine.set_render_context(RenderContext {
        locale: Some("de".to_string()),
        theme_id: Some("cyber".to_string()),
    });
    assert_eq!(
        dropdown_selected(&mut engine, "theme").as_deref(),
        Some("cyber"),
    );
    assert_eq!(
        dropdown_selected(&mut engine, "language").as_deref(),
        Some("de"),
    );
}
