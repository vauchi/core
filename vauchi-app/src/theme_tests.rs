// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

// @internal
#[test]
fn test_parse_hex_valid() {
    parse_hex("#ffffff").expect("expected success");
    assert_eq!(parse_hex("#ffffff").unwrap(), (255, 255, 255));
    assert_eq!(parse_hex("#000000").unwrap(), (0, 0, 0));
    assert_eq!(parse_hex("#1e1e2e").unwrap(), (30, 30, 46));
}

// @internal
#[test]
fn test_parse_hex_invalid() {
    parse_hex("ffffff").expect_err("expected error");
    parse_hex("#fff").expect_err("expected error");
    parse_hex("#gggggg").expect_err("expected error");
}

// @internal
#[test]
// @scenario: accessibility.feature:Sufficient color contrast
fn test_contrast_ratio_black_white() {
    let ratio = contrast_ratio((255, 255, 255), (0, 0, 0));
    assert!(ratio > 20.0, "White on black should have high contrast");
}

// @internal
#[test]
fn test_contrast_ratio_similar_grays() {
    let ratio = contrast_ratio((128, 128, 128), (144, 144, 144));
    assert!(ratio < 2.0, "Similar grays should have low contrast");
}

/// Read generated/themes.json at runtime (not compile time).
/// Returns None if the file doesn't exist (themes repo not checked out or
/// generator not run). Tests that need themes should call this and skip
/// gracefully if None — this prevents compile failures in environments
/// without the themes sibling repo.
fn load_generated_themes() -> Option<Vec<Theme>> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/generated/themes.json");
    let data = std::fs::read(&path).ok()?;
    Some(load_themes_from_json(&data).expect("generated/themes.json must be valid"))
}

// @internal
#[test]
fn test_themes_json_not_empty() {
    let Some(themes) = load_generated_themes() else {
        eprintln!("SKIP: themes/generated/themes.json not found");
        return;
    };
    assert!(!themes.is_empty());
}

// @internal
#[test]
fn test_theme_by_id_found_in_json() {
    let Some(themes) = load_generated_themes() else {
        return;
    };
    let found = themes.iter().find(|t| t.id == "catppuccin-mocha");
    found.expect("expected Some");
}

// @internal
#[test]
fn test_theme_by_id_not_found_in_json() {
    let Some(themes) = load_generated_themes() else {
        return;
    };
    let found = themes.iter().find(|t| t.id == "nonexistent");
    assert!(found.is_none());
}

// @internal
#[test]
fn test_load_themes_from_json_valid() {
    let json = r##"[{
        "id": "test-dark",
        "name": "Test Dark",
        "version": "1.0.0",
        "mode": "dark",
        "colors": {
            "bg-primary": "#1a1a2e",
            "bg-secondary": "#16213e",
            "bg-tertiary": "#0f3460",
            "text-primary": "#eeeeee",
            "text-secondary": "#a0a0a0",
            "accent": "#4fc3f7",
            "accent-dark": "#0288d1",
            "success": "#4caf50",
            "error": "#f44336",
            "warning": "#ff9800",
            "border": "#333333"
        }
    }]"##;
    let themes = load_themes_from_json(json.as_bytes()).unwrap();
    assert_eq!(themes.len(), 1);
    assert_eq!(themes[0].id, "test-dark");
    assert_eq!(themes[0].name, "Test Dark");
    assert_eq!(themes[0].mode, ThemeMode::Dark);
    assert_eq!(themes[0].colors.bg_primary, "#1a1a2e");
    assert_eq!(themes[0].colors.accent, "#4fc3f7");
}

// @internal
#[test]
fn test_load_themes_from_json_multiple_themes() {
    let json = r##"[
        {"id":"a","name":"A","version":"1.0.0","mode":"dark","colors":{"bg-primary":"#000000","bg-secondary":"#111111","bg-tertiary":"#222222","text-primary":"#ffffff","text-secondary":"#cccccc","accent":"#0000ff","accent-dark":"#000099","success":"#00ff00","error":"#ff0000","warning":"#ffff00","border":"#333333"}},
        {"id":"b","name":"B","version":"1.0.0","mode":"light","colors":{"bg-primary":"#ffffff","bg-secondary":"#eeeeee","bg-tertiary":"#dddddd","text-primary":"#000000","text-secondary":"#333333","accent":"#0000ff","accent-dark":"#000099","success":"#00ff00","error":"#ff0000","warning":"#ffff00","border":"#cccccc"}}
    ]"##;
    let themes = load_themes_from_json(json.as_bytes()).unwrap();
    assert_eq!(themes.len(), 2);
    assert_eq!(themes[0].mode, ThemeMode::Dark);
    assert_eq!(themes[1].mode, ThemeMode::Light);
}

// @internal
#[test]
fn test_load_themes_from_json_invalid_json() {
    let result = load_themes_from_json(b"not json");
    result.expect_err("expected error");
}

// @internal
#[test]
fn test_load_themes_from_json_empty_array() {
    let result = load_themes_from_json(b"[]").unwrap();
    assert!(result.is_empty());
}

// @internal
#[test]
fn test_load_themes_from_json_preserves_optional_fields() {
    let json = r##"[{
        "id": "test",
        "name": "Test",
        "version": "1.0.0",
        "author": "Author Name",
        "license": "MIT",
        "source": "https://example.com",
        "mode": "dark",
        "colors": {
            "bg-primary": "#000000","bg-secondary": "#111111","bg-tertiary": "#222222",
            "text-primary": "#ffffff","text-secondary": "#cccccc",
            "accent": "#0000ff","accent-dark": "#000099",
            "success": "#00ff00","error": "#ff0000","warning": "#ffff00","border": "#333333"
        }
    }]"##;
    let themes = load_themes_from_json(json.as_bytes()).unwrap();
    assert_eq!(themes[0].author.as_deref(), Some("Author Name"));
    assert_eq!(themes[0].license.as_deref(), Some("MIT"));
    assert_eq!(themes[0].source.as_deref(), Some("https://example.com"));
}

// @internal
#[test]
fn test_default_theme_returns_valid_dark_theme() {
    let theme = default_theme();
    assert_eq!(theme.id, "catppuccin-mocha");
    assert_eq!(theme.mode, ThemeMode::Dark);
    theme
        .validate_accessibility()
        .expect("Default theme should pass WCAG AA");
}

/// Contract test: validates core can parse the real themes.json from the sibling repo.
/// Set VAUCHI_THEMES_PATH to the path of themes/themes.json to activate.
/// Used by CI's validate-content-contracts job to detect parser/schema drift.
// @internal
#[test]
fn test_load_real_themes_json() {
    let path = match std::env::var("VAUCHI_THEMES_PATH") {
        Ok(p) => p,
        Err(_) => {
            // Also try the default sibling repo path
            let sibling = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../themes/generated/themes.json");
            if sibling.exists() {
                sibling.to_string_lossy().to_string()
            } else {
                // themes/ sibling repo not found and VAUCHI_THEMES_PATH not set — skip
                return;
            }
        }
    };

    let data = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("Failed to read themes.json at {path}: {e}"));
    let themes = load_themes_from_json(&data)
        .unwrap_or_else(|e| panic!("Core parser cannot parse themes.json at {path}: {e}"));

    assert!(
        !themes.is_empty(),
        "themes.json should contain at least one theme"
    );

    // Verify all themes have non-empty required fields
    for theme in &themes {
        assert!(!theme.id.is_empty(), "Theme ID must not be empty");
        assert!(!theme.name.is_empty(), "Theme name must not be empty");
        assert!(
            !theme.colors.bg_primary.is_empty(),
            "Theme {} missing bg_primary",
            theme.id
        );
    }

    // Verified: core parsed all themes from path
    // (assertions above confirm non-empty + required fields)
}

// @internal
#[test]
fn test_design_tokens_default_spacing() {
    let tokens = DesignTokens::default();
    assert_eq!(tokens.spacing.xs, 4);
    assert_eq!(tokens.spacing.sm, 8);
    assert_eq!(tokens.spacing.sm_md, 12);
    assert_eq!(tokens.spacing.md, 16);
    assert_eq!(tokens.spacing.lg, 24);
    assert_eq!(tokens.spacing.xl, 32);
}

// @internal
#[test]
fn test_design_tokens_default_typography() {
    let tokens = DesignTokens::default();
    assert_eq!(tokens.typography.title_size, 24);
    assert_eq!(tokens.typography.subtitle_size, 18);
    assert_eq!(tokens.typography.body_size, 16);
    assert_eq!(tokens.typography.caption_size, 14);
    assert_eq!(tokens.typography.medium_size, 20);
    assert_eq!(tokens.typography.title_line, 30);
    assert_eq!(tokens.typography.subtitle_line, 24);
    assert_eq!(tokens.typography.medium_line, 28);
    assert_eq!(tokens.typography.body_line, 24);
    assert_eq!(tokens.typography.caption_line, 20);
    assert_eq!(tokens.typography.text_scale_percent, 100);
}

// @internal
#[test]
fn test_design_tokens_default_border_radius() {
    let tokens = DesignTokens::default();
    assert_eq!(tokens.border_radius.sm, 4);
    assert_eq!(tokens.border_radius.md, 8);
    assert_eq!(tokens.border_radius.md_lg, 12);
    assert_eq!(tokens.border_radius.lg, 16);
    assert_eq!(tokens.border_radius.chip, 12);
    assert_eq!(tokens.border_radius.card, 20);
    assert_eq!(tokens.border_radius.sheet, 28);
}

// @internal
#[test]
fn test_design_tokens_default_spacing_direction() {
    let tokens = DesignTokens::default();
    assert_eq!(tokens.spacing_direction.content_start, 16);
    assert_eq!(tokens.spacing_direction.content_end, 16);
    assert_eq!(tokens.spacing_direction.list_item_start, 8);
    assert_eq!(tokens.spacing_direction.list_item_end, 8);
    assert_eq!(tokens.spacing_direction.list_item_inline_start, 12);
    assert_eq!(tokens.spacing_direction.list_item_inline_end, 12);
}

// @internal
#[test]
// @scenario: accessibility.feature:Touch targets are large enough
fn test_design_tokens_default_touch_target() {
    let tokens = DesignTokens::default();
    assert_eq!(tokens.touch_target.minimum, 44);
}

// @internal
#[test]
fn test_design_tokens_default_motion() {
    let tokens = DesignTokens::default();
    assert_eq!(tokens.motion.enter_duration_ms, 200);
    assert_eq!(tokens.motion.exit_duration_ms, 150);
    assert_eq!(tokens.motion.emphasis_duration_ms, 300);
}

// @internal
#[test]
fn test_design_tokens_default_font_family() {
    let tokens = DesignTokens::default();
    assert_eq!(tokens.font_family.display, "Bricolage Grotesque");
    assert_eq!(tokens.font_family.body, "Hanken Grotesk");
    assert_eq!(tokens.font_family.mono, "JetBrains Mono");
}

// @internal
#[test]
fn test_design_tokens_default_font_weight() {
    let tokens = DesignTokens::default();
    assert_eq!(tokens.font_weight.regular, 400);
    assert_eq!(tokens.font_weight.medium, 500);
    assert_eq!(tokens.font_weight.semibold, 600);
    assert_eq!(tokens.font_weight.bold, 700);
    assert_eq!(tokens.font_weight.extrabold, 800);
}

// @internal
#[test]
fn test_design_tokens_default_focus() {
    let tokens = DesignTokens::default();
    assert_eq!(tokens.focus.ring_width, 3);
    assert_eq!(tokens.focus.ring_offset, 2);
}

// @internal
#[test]
fn test_design_tokens_serde_roundtrip() {
    let tokens = DesignTokens::default();
    let json = serde_json::to_string(&tokens).unwrap();
    let restored: DesignTokens = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, tokens);
}

// @internal
#[test]
fn test_theme_includes_default_tokens() {
    let theme = default_theme();
    assert_eq!(theme.tokens.spacing.md, 16);
    assert_eq!(theme.tokens.typography.body_size, 16);
    assert_eq!(theme.tokens.border_radius.md, 8);
}

// @internal
#[test]
// @scenario: accessibility.feature:WCAG 2.1 AA compliance on desktop
// @scenario: accessibility.feature:High contrast mode support
fn test_high_contrast_theme_exists_and_accessible() {
    let Some(themes) = load_generated_themes() else {
        return;
    };
    let hc = themes
        .iter()
        .find(|t| t.id == "high-contrast")
        .expect("high-contrast theme must exist in themes.json");

    assert_eq!(hc.name, "High Contrast");
    assert_eq!(hc.mode, ThemeMode::Dark);
    assert_eq!(hc.colors.bg_primary, "#000000");
    assert_eq!(hc.colors.text_primary, "#ffffff");
    assert_eq!(hc.colors.border, "#ffffff");
    hc.validate_accessibility()
        .expect("High-contrast theme must pass WCAG AA");
}

// @internal
#[test]
fn test_theme_json_without_tokens_uses_defaults() {
    let json = r##"[{
        "id": "no-tokens",
        "name": "No Tokens",
        "version": "1.0.0",
        "mode": "dark",
        "colors": {
            "bg-primary": "#000000","bg-secondary": "#111111","bg-tertiary": "#222222",
            "text-primary": "#ffffff","text-secondary": "#cccccc",
            "accent": "#0000ff","accent-dark": "#000099",
            "success": "#00ff00","error": "#ff0000","warning": "#ffff00","border": "#333333"
        }
    }]"##;
    let themes = load_themes_from_json(json.as_bytes()).unwrap();
    assert_eq!(themes[0].tokens, DesignTokens::default());
}

// ── Hot-reload store tests (ADR-038 Amendment 2) ──────────────
// The stores are process-global; serialize mutating tests so parallel
// nextest threads don't interfere (mirror i18n's I18N_TEST_LOCK), and
// reset to the unloaded state around each so other tests see defaults.
static STORE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn reset_design_stores() {
    *super::DESIGN_TOKENS_STORE.write().unwrap() = None;
    *super::THEME_STORE.write().unwrap() = None;
}

// @internal
#[test]
fn active_design_tokens_falls_back_to_default_when_unloaded() {
    let _g = STORE_TEST_LOCK.lock().unwrap();
    reset_design_stores();
    assert_eq!(active_design_tokens(), DesignTokens::default());
    assert!(!design_tokens_loaded());
}

// @internal
#[test]
fn load_design_tokens_then_active_returns_loaded() {
    let _g = STORE_TEST_LOCK.lock().unwrap();
    reset_design_stores();
    let mut t = DesignTokens::default();
    t.spacing.md = 99; // distinct from the default 16
    let json = serde_json::to_vec(&t).unwrap();
    load_design_tokens_from_bytes(&json).unwrap();
    assert!(design_tokens_loaded());
    assert_eq!(active_design_tokens().spacing.md, 99);
    reset_design_stores();
}

// @internal
#[test]
fn malformed_tokens_json_errors_without_poisoning() {
    let _g = STORE_TEST_LOCK.lock().unwrap();
    reset_design_stores();
    assert!(load_design_tokens_from_bytes(b"not json").is_err());
    // Store untouched -> reads still succeed and return the default.
    assert_eq!(active_design_tokens(), DesignTokens::default());
    reset_design_stores();
}

// @internal
#[test]
fn active_themes_falls_back_to_bundled_when_unloaded() {
    let _g = STORE_TEST_LOCK.lock().unwrap();
    reset_design_stores();
    let themes = active_themes();
    assert!(!themes.is_empty());
    assert!(themes.iter().any(|t| t.id == default_theme().id));
}

// @internal
#[test]
fn load_themes_then_active_returns_loaded() {
    let _g = STORE_TEST_LOCK.lock().unwrap();
    reset_design_stores();
    let json = br##"[{
        "id": "hot-loaded","name": "Hot","version": "1.0.0","mode": "dark",
        "colors": {
            "bg-primary": "#010203","bg-secondary": "#111111","bg-tertiary": "#222222",
            "text-primary": "#ffffff","text-secondary": "#cccccc",
            "accent": "#0000ff","accent-dark": "#000099",
            "success": "#00ff00","error": "#ff0000","warning": "#ffff00","border": "#333333"
        }
    }]"##;
    load_themes_from_bytes(json).unwrap();
    let themes = active_themes();
    assert_eq!(themes.len(), 1);
    assert_eq!(themes[0].id, "hot-loaded");
    reset_design_stores();
}

// @internal
#[test]
fn reloaded_tokens_flow_into_new_screenmodels() {
    let _g = STORE_TEST_LOCK.lock().unwrap();
    reset_design_stores();
    let mut t = DesignTokens::default();
    t.spacing.md = 99;
    load_design_tokens_from_bytes(&serde_json::to_vec(&t).unwrap()).unwrap();
    let screen = crate::ui::ScreenModel::new("s", "T", Vec::new(), Vec::new());
    assert_eq!(
        screen.tokens.spacing.md, 99,
        "a reloaded tokens.json must flow into emitted ScreenModels"
    );
    reset_design_stores();
}
