// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! "Follow system" paints the default theme for the OS light or dark
//! setting, which the shell reports with its presentation environment
//! (vauchi/private#814). A pick overrides it. The theme store is
//! process-global; nextest runs each test in its own process.

use vauchi_app::ui::{AppEngine, RenderContext};
use vauchi_core::api::Vauchi;
use vauchi_core::{ColorScheme, Command, Event, InputMode, MotionPreference};

const LIGHT_BACKGROUND: &str = "#fffff0";
const DARK_BACKGROUND: &str = "#00000f";
const PICKED_BACKGROUND: &str = "#282a36";

const CATALOG: &str = r##"[
    {"id": "catppuccin-latte", "background": "#fffff0", "mode": "light"},
    {"id": "catppuccin-mocha", "background": "#00000f", "mode": "dark"},
    {"id": "picked", "background": "#282a36", "mode": "dark"}
]"##;

fn catalog_json() -> Vec<u8> {
    let entries: Vec<serde_json::Value> = serde_json::from_str(CATALOG).unwrap();
    let themes: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|entry| {
            serde_json::json!({
                "id": entry["id"], "name": entry["id"], "version": "1.0.0",
                "mode": entry["mode"],
                "colors": {
                    "bg-primary": entry["background"], "bg-secondary": "#111111",
                    "bg-tertiary": "#222222", "text-primary": "#777777",
                    "text-secondary": "#888888", "accent": "#0000ff",
                    "accent-dark": "#000099", "success": "#00ff00", "error": "#ff0000",
                    "warning": "#ffff00", "border": "#333333"
                }
            })
        })
        .collect();
    serde_json::to_vec(&themes).unwrap()
}

fn engine(theme_id: Option<&str>) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory core");
    vauchi.create_identity("Alice").expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine
        .reload_design_assets(Some(&catalog_json()), None)
        .expect("the test catalogue loads");
    engine.set_render_context(RenderContext {
        theme_id: theme_id.map(str::to_string),
        ..RenderContext::default()
    });
    engine.initial_commands().expect("initial commands");
    engine
}

fn environment(color_scheme: Option<ColorScheme>) -> Event {
    Event::PresentationEnvironmentChanged {
        available_width: 400,
        available_height: 800,
        input_modes: vec![InputMode::Touch],
        motion: MotionPreference::Full,
        color_scheme,
    }
}

fn backgrounds(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.tokens.colors.background.clone()),
            _ => None,
        })
        .collect()
}

fn current_background(engine: &mut AppEngine) -> String {
    backgrounds(&engine.initial_commands().expect("initial commands"))
        .first()
        .cloned()
        .expect("a surface")
}

// @internal
#[test]
fn follow_system_paints_the_light_default_on_a_light_system() {
    let mut engine = engine(None);
    engine
        .dispatch(environment(Some(ColorScheme::Light)))
        .expect("environment");

    assert_eq!(current_background(&mut engine), LIGHT_BACKGROUND);
}

// @internal
#[test]
fn follow_system_paints_the_dark_default_on_a_dark_system() {
    let mut engine = engine(None);
    engine
        .dispatch(environment(Some(ColorScheme::Dark)))
        .expect("environment");

    assert_eq!(current_background(&mut engine), DARK_BACKGROUND);
}

// @internal
#[test]
fn a_shell_that_reports_no_scheme_gets_the_dark_default() {
    let mut engine = engine(None);
    engine.dispatch(environment(None)).expect("environment");

    assert_eq!(current_background(&mut engine), DARK_BACKGROUND);
}

// @internal
#[test]
fn switching_the_system_to_light_repaints_at_once() {
    let mut engine = engine(None);
    engine
        .dispatch(environment(Some(ColorScheme::Dark)))
        .expect("environment");

    let commands = engine
        .dispatch(environment(Some(ColorScheme::Light)))
        .expect("environment");

    assert_eq!(backgrounds(&commands), vec![LIGHT_BACKGROUND.to_string()]);
}

// @internal
#[test]
fn a_pick_overrides_the_system_scheme_and_a_switch_repaints_nothing() {
    let mut engine = engine(Some("picked"));
    engine
        .dispatch(environment(Some(ColorScheme::Dark)))
        .expect("environment");

    let commands = engine
        .dispatch(environment(Some(ColorScheme::Light)))
        .expect("environment");

    assert_eq!(backgrounds(&commands), Vec::<String>::new());
    assert_eq!(current_background(&mut engine), PICKED_BACKGROUND);
}

// @internal
#[test]
fn the_scheme_survives_a_render_context_push() {
    let mut engine = engine(None);
    engine
        .dispatch(environment(Some(ColorScheme::Light)))
        .expect("environment");

    engine.set_render_context(RenderContext::default());

    assert_eq!(current_background(&mut engine), LIGHT_BACKGROUND);
}

// @internal
#[test]
fn a_light_scheme_on_the_wire_decodes_and_its_absence_decodes_as_none() {
    let with = serde_json::json!({"PresentationEnvironmentChanged": {
        "available_width": 400, "available_height": 800,
        "input_modes": ["touch"], "motion": "full", "color_scheme": "light"
    }});
    let without = serde_json::json!({"PresentationEnvironmentChanged": {
        "available_width": 400, "available_height": 800,
        "input_modes": ["touch"], "motion": "full"
    }});

    assert_eq!(
        serde_json::from_value::<Event>(with).expect("decodes"),
        environment(Some(ColorScheme::Light))
    );
    assert_eq!(
        serde_json::from_value::<Event>(without).expect("decodes"),
        environment(None)
    );
}
