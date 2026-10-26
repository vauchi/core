// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Text Core paints is readable on the surfaces it is drawn on, in every
//! theme: WCAG AA's 4.5:1, whatever the theme data says
//! (vauchi/private#814). A shell paints Core's colours as sent, so a
//! low-contrast theme would otherwise fail every shell's accessibility
//! audit. The theme store is process-global; nextest runs each test in
//! its own process, so the themes loaded here leak nowhere.

use vauchi_app::theme::bundled_themes;
use vauchi_app::ui::{AppEngine, RenderContext};
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, PresentationColors};

const AA: f64 = 4.5;

fn rgb(hex: &str) -> (f64, f64, f64) {
    let channel = |i: usize| f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap()) / 255.0;
    (channel(1), channel(3), channel(5))
}

fn luminance(hex: &str) -> f64 {
    let linear = |c: f64| {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = rgb(hex);
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn colors_for(engine: &mut AppEngine, theme_id: &str) -> PresentationColors {
    engine.set_render_context(RenderContext {
        theme_id: Some(theme_id.to_string()),
        ..RenderContext::default()
    });
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

fn engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory core");
    vauchi.create_identity("Alice").expect("identity");
    AppEngine::new(vauchi)
}

fn unreadable(colors: &PresentationColors) -> Vec<String> {
    let mut found = Vec::new();
    for (role, text) in [("text", &colors.text), ("text_muted", &colors.text_muted)] {
        for (surface_role, surface) in [
            ("background", &colors.background),
            ("surface", &colors.surface),
        ] {
            let ratio = contrast(text, surface);
            if ratio < AA {
                found.push(format!(
                    "{role} {text} on {surface_role} {surface}: {ratio:.2}"
                ));
            }
        }
    }
    found
}

// @internal
#[test]
fn every_bundled_theme_paints_text_readable_on_its_surfaces() {
    let themes = bundled_themes();
    assert!(themes.len() > 1, "the bundled catalogue is loaded");
    let mut engine = engine();

    let failures: Vec<String> = themes
        .iter()
        .flat_map(|theme| {
            unreadable(&colors_for(&mut engine, &theme.id))
                .into_iter()
                .map(move |finding| format!("{}: {finding}", theme.id))
        })
        .collect();

    assert_eq!(failures, Vec::<String>::new());
}

const LOW_CONTRAST_THEME: &[u8] = br##"[{
    "id": "low","name": "Low","version": "1.0.0","mode": "light",
    "colors": {
        "bg-primary": "#f0f0f0","bg-secondary": "#e0e0e0","bg-tertiary": "#d0d0d0",
        "text-primary": "#909090","text-secondary": "#a8a8a8",
        "accent": "#7aa2f7","accent-dark": "#5a82d7",
        "success": "#50fa7b","error": "#ff5555","warning": "#ffb86c","border": "#c0c0c0",
        "text-on-accent": "#8ab2ff"
    }
}]"##;

// @internal
#[test]
fn a_theme_with_faint_text_is_painted_readable() {
    let mut engine = engine();
    engine
        .reload_design_assets(Some(LOW_CONTRAST_THEME), None)
        .expect("the test theme loads");

    let colors = colors_for(&mut engine, "low");

    assert_eq!(unreadable(&colors), Vec::<String>::new());
    assert_eq!(
        colors.background, "#f0f0f0",
        "surfaces stay as the theme set them"
    );
}

// @internal
#[test]
fn text_on_accent_is_readable_on_the_accent() {
    let mut engine = engine();
    engine
        .reload_design_assets(Some(LOW_CONTRAST_THEME), None)
        .expect("the test theme loads");

    let colors = colors_for(&mut engine, "low");
    let on_accent = colors
        .text_on_accent
        .expect("the theme sets text-on-accent");

    assert!(
        contrast(&on_accent, &colors.accent) >= AA,
        "{on_accent} on {}: {:.2}",
        colors.accent,
        contrast(&on_accent, &colors.accent)
    );
}
