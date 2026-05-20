// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! S6 of the settings-storage-by-sensitivity plan
//! (`_private/docs/planning/todo/2026-05-16-settings-storage-by-sensitivity-plan.md`):
//! the Settings ScreenModel's `theme` and `language` Dropdown
//! `selected` values derive exclusively from `engine.render_context()`.
//! The legacy `vauchi.app_preferences()` fallback is retired; absent
//! RenderContext fields render the reserved "follow_system" option
//! per ADR-047 (absence-is-follow-system semantic).
//!
//! These tests pin the read path. The intercept contract (Settings
//! dropdown picks update RenderContext only — no vault writes) is
//! covered below by the `*_updates_render_context` tests.

use vauchi_app::ui::{AppEngine, AppScreen, Component, RenderContext, UserAction, WorkflowEngine};
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

    // No RenderContext push yet → renders the reserved "follow_system" id.
    assert_eq!(
        dropdown_selected(&mut engine, "theme").as_deref(),
        Some("follow_system"),
        "without a RenderContext push the dropdown must render the \
         reserved follow_system id (ADR-047 absence-is-follow-system)"
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
         the Settings theme dropdown's `selected` must reflect the pushed value"
    );
}

// @internal
#[test]
fn language_dropdown_selected_reflects_pushed_render_context() {
    let mut engine = engine_with_identity();

    assert_eq!(
        dropdown_selected(&mut engine, "language").as_deref(),
        Some("follow_system"),
    );

    engine.set_render_context(RenderContext {
        locale: Some("de".to_string()),
        theme_id: None,
    });

    assert_eq!(
        dropdown_selected(&mut engine, "language").as_deref(),
        Some("de"),
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

    assert_eq!(
        dropdown_selected(&mut engine, "language").as_deref(),
        Some("fr"),
    );
    // theme is None in RenderContext → renders follow_system (no vault fallback).
    assert_eq!(
        dropdown_selected(&mut engine, "theme").as_deref(),
        Some("follow_system"),
    );
}

// @internal
#[test]
fn theme_dropdown_selection_updates_render_context() {
    // S6 contract: when the user picks a theme via the Settings
    // dropdown, RenderContext updates. The vault write path was
    // retired; the frontend owns persistence (UserDefaults /
    // SharedPreferences) and pushes back via setRenderContextJson.
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Settings);
    assert_eq!(engine.render_context().theme_id, None);

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "theme".to_string(),
        item_id: "cyber".to_string(),
    });

    assert_eq!(
        engine.render_context().theme_id.as_deref(),
        Some("cyber"),
        "intercept must update RenderContext.theme_id when the user \
         picks a non-default theme from the Settings dropdown"
    );
}

// @internal
#[test]
fn language_dropdown_selection_updates_render_context() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Settings);
    assert_eq!(engine.render_context().locale, None);

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "language".to_string(),
        item_id: "de".to_string(),
    });

    assert_eq!(engine.render_context().locale.as_deref(), Some("de"));
}

// @internal
#[test]
fn follow_system_selection_clears_render_context_field() {
    // The reserved "follow_system" id means "core/OS picks". In
    // RenderContext that maps to None (the absence-is-follow-system
    // semantic per ADR-047).
    let mut engine = engine_with_identity();
    engine.set_render_context(RenderContext {
        locale: Some("de".to_string()),
        theme_id: Some("cyber".to_string()),
    });
    engine.navigate_to(AppScreen::Settings);

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "theme".to_string(),
        item_id: "follow_system".to_string(),
    });

    assert_eq!(
        engine.render_context().theme_id,
        None,
        "follow_system maps to None in RenderContext per ADR-047"
    );
    // Other field untouched.
    assert_eq!(engine.render_context().locale.as_deref(), Some("de"));
}
