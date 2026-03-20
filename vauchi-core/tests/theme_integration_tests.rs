// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Theme Integration Tests
//!
//! Integration tests verifying theme system against Gherkin scenarios.
//! Feature file: features/theming.feature
//!
//! These tests verify:
//! - Theme structure and validation
//! - Contrast ratio accessibility checks
//! - Bundled themes (default, Catppuccin, Dracula, Nord, Solarized, Gruvbox)
//! - Theme persistence and selection

mod common;

use common::helpers::{all_themes, theme_by_id};
use vauchi_core::theme::{DesignTokens, Theme, ThemeColors, ThemeMode, validate_hex_color};

// ============================================================
// Theme Structure
// Feature: theming.feature @selection
// ============================================================

/// Test: Theme has required fields
// @scenario: theming:Theme file contains required colors
#[test]
fn test_theme_has_required_fields() {
    let themes = all_themes();
    assert!(!themes.is_empty(), "Should have bundled themes");

    for theme in &themes {
        assert!(!theme.id.is_empty(), "Theme should have ID");
        assert!(!theme.name.is_empty(), "Theme should have name");
        assert!(!theme.version.is_empty(), "Theme should have version");
    }
}

/// Test: ThemeColors has all required color fields
// @scenario: theming:Theme file contains required colors
#[test]
fn test_theme_colors_complete() {
    let themes = all_themes();

    for theme in &themes {
        let colors = &theme.colors;
        assert!(
            validate_hex_color(&colors.bg_primary).is_ok(),
            "bg_primary should be valid hex"
        );
        assert!(
            validate_hex_color(&colors.bg_secondary).is_ok(),
            "bg_secondary should be valid hex"
        );
        assert!(
            validate_hex_color(&colors.text_primary).is_ok(),
            "text_primary should be valid hex"
        );
        assert!(
            validate_hex_color(&colors.accent).is_ok(),
            "accent should be valid hex"
        );
        assert!(
            validate_hex_color(&colors.success).is_ok(),
            "success should be valid hex"
        );
        assert!(
            validate_hex_color(&colors.error).is_ok(),
            "error should be valid hex"
        );
    }
}

// ============================================================
// Theme Mode
// Feature: theming.feature @selection
// ============================================================

/// Test: Themes have correct mode (light/dark)
// @scenario: theming:Theme file specifies light/dark mode
#[test]
fn test_theme_modes() {
    let themes = all_themes();

    let dark_themes: Vec<_> = themes
        .iter()
        .filter(|t| t.mode == ThemeMode::Dark)
        .collect();
    let light_themes: Vec<_> = themes
        .iter()
        .filter(|t| t.mode == ThemeMode::Light)
        .collect();

    assert!(!dark_themes.is_empty(), "Should have dark themes");
    assert!(!light_themes.is_empty(), "Should have light themes");
}

/// Test: Default theme is dark mode
/// Feature: theming.feature @selection
/// Scenario: Default theme on fresh install
// @scenario: theming:Default theme on fresh install
#[test]
fn test_default_theme_is_dark() {
    let default = theme_by_id("default-dark");
    assert!(default.is_some(), "Should have default-dark theme");
    assert_eq!(default.unwrap().mode, ThemeMode::Dark);
}

// ============================================================
// Accessibility - Contrast Ratios
// Feature: theming.feature @accessibility
// ============================================================

/// Test: All themes pass WCAG contrast requirements
// @scenario: theming:Theme colors meet minimum contrast
#[test]
fn test_all_themes_accessible() {
    let themes = all_themes();

    for theme in &themes {
        let result = theme.validate_accessibility();
        assert!(
            result.is_ok(),
            "Theme {} should pass accessibility check: {:?}",
            theme.id,
            result.err()
        );
    }
}

/// Test: Contrast ratio calculation
// @scenario: theming:Theme colors meet minimum contrast
#[test]
fn test_contrast_ratio_calculation() {
    // White on black should have high contrast
    let white = "#ffffff";
    let black = "#000000";

    let colors = ThemeColors {
        bg_primary: black.to_string(),
        bg_secondary: black.to_string(),
        bg_tertiary: black.to_string(),
        text_primary: white.to_string(),
        text_secondary: white.to_string(),
        accent: white.to_string(),
        accent_dark: white.to_string(),
        success: white.to_string(),
        error: white.to_string(),
        warning: white.to_string(),
        border: white.to_string(),
    };

    let theme = Theme {
        id: "test".to_string(),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        author: None,
        license: None,
        source: None,
        mode: ThemeMode::Dark,
        colors,
        tokens: DesignTokens::default(),
    };

    assert!(
        theme.validate_accessibility().is_ok(),
        "White on black should pass"
    );
}

/// Test: Low contrast should fail validation
// @scenario: theming:Theme colors meet minimum contrast
#[test]
fn test_low_contrast_fails() {
    let gray1 = "#808080";
    let gray2 = "#909090";

    let colors = ThemeColors {
        bg_primary: gray1.to_string(),
        bg_secondary: gray1.to_string(),
        bg_tertiary: gray1.to_string(),
        text_primary: gray2.to_string(),
        text_secondary: gray2.to_string(),
        accent: gray2.to_string(),
        accent_dark: gray2.to_string(),
        success: gray2.to_string(),
        error: gray2.to_string(),
        warning: gray2.to_string(),
        border: gray2.to_string(),
    };

    let theme = Theme {
        id: "test".to_string(),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        author: None,
        license: None,
        source: None,
        mode: ThemeMode::Dark,
        colors,
        tokens: DesignTokens::default(),
    };

    assert!(
        theme.validate_accessibility().is_err(),
        "Low contrast should fail"
    );
}

// ============================================================
// Bundled Themes
// Feature: theming.feature @catppuccin, @dracula, @nord, etc.
// ============================================================

/// Test: Default themes exist
// @scenario: theming:Bundled themes always available
// @scenario: theming:Default theme on fresh install
#[test]
fn test_default_themes_exist() {
    theme_by_id("default-dark").expect("expected Some");
    theme_by_id("default-light").expect("expected Some");
}

/// Test: Catppuccin themes exist
/// Feature: theming.feature @catppuccin
// @scenario: theming:Catppuccin flavor themes available
#[test]
fn test_catppuccin_themes_exist() {
    assert!(
        theme_by_id("catppuccin-mocha").is_some(),
        "Should have Catppuccin Mocha"
    );
    assert!(
        theme_by_id("catppuccin-latte").is_some(),
        "Should have Catppuccin Latte"
    );
}

/// Test: Catppuccin Mocha has correct colors
/// Feature: theming.feature @catppuccin @dark
// @scenario: theming:Apply Catppuccin Mocha (dark)
#[test]
fn test_catppuccin_mocha_colors() {
    let theme = theme_by_id("catppuccin-mocha").unwrap();

    assert_eq!(theme.mode, ThemeMode::Dark);
    assert_eq!(theme.colors.bg_primary, "#1e1e2e");
    assert_eq!(theme.colors.text_primary, "#cdd6f4");
    assert_eq!(theme.colors.accent, "#89b4fa");
}

/// Test: Catppuccin Latte has correct colors
/// Feature: theming.feature @catppuccin @light
// @scenario: theming:Apply Catppuccin Latte (light)
#[test]
fn test_catppuccin_latte_colors() {
    let theme = theme_by_id("catppuccin-latte").unwrap();

    assert_eq!(theme.mode, ThemeMode::Light);
    assert_eq!(theme.colors.bg_primary, "#eff1f5");
    assert_eq!(theme.colors.text_primary, "#4c4f69");
}

/// Test: Dracula theme exists and has correct colors
/// Feature: theming.feature (implied)
// @scenario: theming:Apply Dracula theme
#[test]
fn test_dracula_theme() {
    let theme = theme_by_id("dracula").unwrap();

    assert_eq!(theme.mode, ThemeMode::Dark);
    assert_eq!(theme.colors.bg_primary, "#282a36");
    assert_eq!(theme.colors.text_primary, "#f8f8f2");
    assert_eq!(theme.colors.accent, "#bd93f9");
}

/// Test: Nord theme exists and has correct colors
// @scenario: theming:Apply Nord theme
#[test]
fn test_nord_theme() {
    let theme = theme_by_id("nord").unwrap();

    assert_eq!(theme.mode, ThemeMode::Dark);
    assert_eq!(theme.colors.bg_primary, "#2e3440");
    assert_eq!(theme.colors.text_primary, "#eceff4");
}

/// Test: Solarized themes exist
// @scenario: theming:Solarized themes available
#[test]
fn test_solarized_themes() {
    theme_by_id("solarized-dark").expect("expected Some");
    theme_by_id("solarized-light").expect("expected Some");
}

/// Test: Gruvbox themes exist
// @scenario: theming:Gruvbox themes available
#[test]
fn test_gruvbox_themes() {
    theme_by_id("gruvbox-dark").expect("expected Some");
    theme_by_id("gruvbox-light").expect("expected Some");
}

// ============================================================
// Theme Count and Listing
// ============================================================

/// Test: Have expected number of bundled themes
// @scenario: theming:Select theme from settings
#[test]
fn test_bundled_theme_count() {
    let themes = all_themes();
    // default-dark, default-light, catppuccin-mocha, catppuccin-latte,
    // dracula, nord, solarized-dark, solarized-light, gruvbox-dark, gruvbox-light
    assert!(themes.len() >= 10, "Should have at least 10 bundled themes");
}

/// Test: All themes have unique IDs
// @scenario: theming:Select theme from settings
#[test]
fn test_unique_theme_ids() {
    let themes = all_themes();
    let mut ids: Vec<_> = themes.iter().map(|t| &t.id).collect();
    let original_len = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), original_len, "All theme IDs should be unique");
}

// ============================================================
// Hex Color Validation
// ============================================================

/// Test: Valid hex colors pass validation
// @scenario: theming:Theme file contains required colors
#[test]
fn test_valid_hex_colors() {
    validate_hex_color("#ffffff").expect("expected success");
    validate_hex_color("#000000").expect("expected success");
    validate_hex_color("#1e1e2e").expect("expected success");
    validate_hex_color("#ABCDEF").expect("expected success");
}

/// Test: Invalid hex colors fail validation
// @scenario: theming:Invalid theme file rejected
#[test]
fn test_invalid_hex_colors() {
    assert!(validate_hex_color("ffffff").is_err(), "Missing #");
    assert!(validate_hex_color("#fff").is_err(), "Too short");
    assert!(validate_hex_color("#gggggg").is_err(), "Invalid chars");
    assert!(validate_hex_color("").is_err(), "Empty");
}

// ============================================================
// Theme Serialization
// ============================================================

/// Test: Themes can be serialized to JSON
// @scenario: theming:Theme file contains required colors
#[test]
fn test_theme_serialization() {
    let theme = theme_by_id("catppuccin-mocha").unwrap();

    let json = serde_json::to_string(&theme).expect("Should serialize");
    assert!(json.contains("catppuccin-mocha"));
    assert!(json.contains("#1e1e2e"));

    let restored: Theme = serde_json::from_str(&json).expect("Should deserialize");
    assert_eq!(restored.id, theme.id);
    assert_eq!(restored.colors.bg_primary, theme.colors.bg_primary);
}

// ============================================================
// Attribution
// ============================================================

/// Test: Third-party themes have attribution
// @scenario: theming:Theme file contains required colors
#[test]
fn test_theme_attribution() {
    let catppuccin = theme_by_id("catppuccin-mocha").unwrap();
    catppuccin.author.expect("expected Some");
    catppuccin.license.expect("expected Some");
    catppuccin.source.expect("expected Some");

    let dracula = theme_by_id("dracula").unwrap();
    dracula.author.expect("expected Some");
}
