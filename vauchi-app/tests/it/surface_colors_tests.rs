// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Every surface carries the active theme's colours, so a shell paints a
//! pick without a theme store of its own (vauchi/private#814). The theme
//! store is process-global; nextest runs each test in its own process, so
//! the theme loaded here leaks nowhere.

use vauchi_app::ui::{AppEngine, AppScreen, RenderContext, UserAction, WorkflowEngine};
use vauchi_core::api::{AuthMode, Vauchi};
use vauchi_core::{Command, PresentationColors};

const PICKED_THEME: &[u8] = br##"[{
    "id": "picked","name": "Picked","version": "1.0.0","mode": "dark",
    "colors": {
        "bg-primary": "#282a36","bg-secondary": "#21222c","bg-tertiary": "#44475a",
        "text-primary": "#f8f8f2","text-secondary": "#bfbfbf",
        "accent": "#bd93f9","accent-dark": "#9580ff",
        "success": "#50fa7b","error": "#ff5555","warning": "#ffb86c","border": "#6272a4",
        "surface-input": "#191a21","surface-muted": "#343746",
        "text-on-accent": "#282a36","focus-ring": "#ff79c6"
    }
}]"##;

const PASSWORD: &str = "correct horse battery";
const DURESS_PIN: &str = "654321";

fn picked_colors() -> PresentationColors {
    PresentationColors {
        background: "#282a36".into(),
        surface: "#21222c".into(),
        text: "#f8f8f2".into(),
        text_muted: "#bfbfbf".into(),
        accent: "#bd93f9".into(),
        border: "#6272a4".into(),
        success: "#50fa7b".into(),
        warning: "#ffb86c".into(),
        error: "#ff5555".into(),
        surface_input: Some("#191a21".into()),
        surface_muted: Some("#343746".into()),
        surface_elevated: None,
        text_on_accent: Some("#282a36".into()),
        focus_ring: Some("#ff79c6".into()),
    }
}

fn engine_with_picked_theme(vauchi: Vauchi) -> AppEngine {
    let mut engine = AppEngine::new(vauchi);
    engine
        .reload_design_assets(Some(PICKED_THEME), None)
        .expect("the test theme loads");
    engine
}

fn identity() -> Vauchi {
    let mut vauchi = Vauchi::in_memory().expect("in-memory core");
    vauchi.create_identity("Alice").expect("identity");
    vauchi
}

fn surface_colors(engine: &mut AppEngine) -> PresentationColors {
    engine
        .initial_commands()
        .expect("initial commands")
        .into_iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(*surface.tokens.colors),
            _ => None,
        })
        .expect("a surface")
}

fn push_theme(engine: &mut AppEngine, theme_id: &str) {
    engine.set_render_context(RenderContext {
        theme_id: Some(theme_id.to_string()),
        ..RenderContext::default()
    });
}

// @internal
#[test]
fn a_theme_picked_in_settings_colours_the_next_surface() {
    let mut engine = engine_with_picked_theme(identity());
    engine.navigate_to(AppScreen::SettingsAppearance);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "theme".to_string(),
        item_id: "picked".to_string(),
    });

    assert_eq!(surface_colors(&mut engine), picked_colors());
}

// @internal
#[test]
fn before_any_pick_surfaces_carry_the_default_theme() {
    let mut engine = AppEngine::new(identity());

    let colors = surface_colors(&mut engine);

    assert_eq!(colors.background, "#1e1e2e");
    assert_eq!(colors.text, "#cdd6f4");
    assert_eq!(colors.accent, "#89b4fa");
}

// @internal
#[test]
fn a_pick_missing_from_the_catalog_falls_back_to_the_default_theme() {
    let mut engine = AppEngine::new(identity());
    push_theme(&mut engine, "removed-theme");

    assert_eq!(surface_colors(&mut engine).background, "#1e1e2e");
}

fn colors_after_unlocking_with(secret: &str) -> (AuthMode, PresentationColors) {
    let mut vauchi = identity();
    vauchi.setup_app_password(PASSWORD).expect("app password");
    vauchi.authenticate(PASSWORD).expect("unlock");
    vauchi
        .setup_duress_password(DURESS_PIN)
        .expect("duress PIN");
    let mode = vauchi.authenticate(secret).expect("unlock");
    let mut engine = engine_with_picked_theme(vauchi);
    push_theme(&mut engine, "picked");
    (mode, surface_colors(&mut engine))
}

// @scenario: duress_mode.feature :: Duress mode looks identical to normal mode
#[test]
fn a_duress_unlock_shows_the_same_colours_as_a_normal_unlock() {
    let (normal_mode, normal) = colors_after_unlocking_with(PASSWORD);
    let (duress_mode, duress) = colors_after_unlocking_with(DURESS_PIN);

    assert_eq!(
        (normal_mode, duress_mode),
        (AuthMode::Normal, AuthMode::Duress)
    );
    assert_eq!(normal, picked_colors());
    assert_eq!(duress, normal);
}
