// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Theme System
//!
//! Provides color theming with popular open-source themes.
//! Supports dark/light modes and WCAG accessibility validation.
//!
//! Feature file: features/theming.feature

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Theme validation errors
#[derive(Debug, Error)]
pub enum ThemeError {
    #[error("Invalid hex color: {0}")]
    InvalidHexColor(String),

    #[error("Insufficient contrast ratio: {actual:.2} (required: {required:.2})")]
    InsufficientContrast { actual: f64, required: f64 },

    #[error("Theme not found: {0}")]
    NotFound(String),

    #[error("Invalid theme JSON: {0}")]
    InvalidThemeJson(String),
}

/// Theme mode (light or dark)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
}

/// Core color definitions for a theme
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeColors {
    #[serde(rename = "bg-primary")]
    pub bg_primary: String,
    #[serde(rename = "bg-secondary")]
    pub bg_secondary: String,
    #[serde(rename = "bg-tertiary")]
    pub bg_tertiary: String,
    #[serde(rename = "text-primary")]
    pub text_primary: String,
    #[serde(rename = "text-secondary")]
    pub text_secondary: String,
    pub accent: String,
    #[serde(rename = "accent-dark")]
    pub accent_dark: String,
    pub success: String,
    pub error: String,
    pub warning: String,
    pub border: String,
}

/// Design tokens for consistent cross-platform rendering.
///
/// Provides spacing, typography, and border radius values that
/// all platform clients use for layout consistency.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignTokens {
    pub spacing: SpacingTokens,
    pub typography: TypographyTokens,
    pub border_radius: BorderRadiusTokens,
}

/// Spacing scale for margins, padding, and gaps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpacingTokens {
    pub xs: u16,
    pub sm: u16,
    pub md: u16,
    pub lg: u16,
    pub xl: u16,
}

/// Font size tokens for text hierarchy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypographyTokens {
    pub title_size: u16,
    pub subtitle_size: u16,
    pub body_size: u16,
    pub caption_size: u16,
}

/// Border radius tokens for rounded corners.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BorderRadiusTokens {
    pub sm: u16,
    pub md: u16,
    pub lg: u16,
}

impl Default for DesignTokens {
    fn default() -> Self {
        Self {
            spacing: SpacingTokens {
                xs: 4,
                sm: 8,
                md: 16,
                lg: 24,
                xl: 32,
            },
            typography: TypographyTokens {
                title_size: 24,
                subtitle_size: 18,
                body_size: 16,
                caption_size: 14,
            },
            border_radius: BorderRadiusTokens {
                sm: 4,
                md: 8,
                lg: 16,
            },
        }
    }
}

/// A complete theme definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub mode: ThemeMode,
    pub colors: ThemeColors,
    /// Design tokens for layout consistency. Falls back to defaults if absent in JSON.
    #[serde(default)]
    pub tokens: DesignTokens,
}

impl Theme {
    /// Validate theme has sufficient contrast ratios for accessibility.
    /// Uses WCAG 2.0 AA standard (4.5:1 for normal text).
    pub fn validate_accessibility(&self) -> Result<(), ThemeError> {
        let bg = parse_hex(&self.colors.bg_primary)?;
        let text = parse_hex(&self.colors.text_primary)?;

        let ratio = contrast_ratio(bg, text);
        if ratio < 4.5 {
            return Err(ThemeError::InsufficientContrast {
                actual: ratio,
                required: 4.5,
            });
        }

        Ok(())
    }
}

/// Validate a hex color string
pub fn validate_hex_color(color: &str) -> Result<(), ThemeError> {
    parse_hex(color).map(|_| ())
}

/// Parse a hex color string to RGB tuple
fn parse_hex(color: &str) -> Result<(u8, u8, u8), ThemeError> {
    if !color.starts_with('#') || color.len() != 7 {
        return Err(ThemeError::InvalidHexColor(color.to_string()));
    }

    let r = u8::from_str_radix(&color[1..3], 16)
        .map_err(|_| ThemeError::InvalidHexColor(color.to_string()))?;
    let g = u8::from_str_radix(&color[3..5], 16)
        .map_err(|_| ThemeError::InvalidHexColor(color.to_string()))?;
    let b = u8::from_str_radix(&color[5..7], 16)
        .map_err(|_| ThemeError::InvalidHexColor(color.to_string()))?;

    Ok((r, g, b))
}

/// Calculate relative luminance of a color (WCAG formula)
fn relative_luminance(color: (u8, u8, u8)) -> f64 {
    let (r, g, b) = color;

    let r = srgb_to_linear(r as f64 / 255.0);
    let g = srgb_to_linear(g as f64 / 255.0);
    let b = srgb_to_linear(b as f64 / 255.0);

    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Convert sRGB to linear RGB
fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Calculate WCAG contrast ratio between two colors
fn contrast_ratio(c1: (u8, u8, u8), c2: (u8, u8, u8)) -> f64 {
    let l1 = relative_luminance(c1);
    let l2 = relative_luminance(c2);
    let (lighter, darker) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Load themes from JSON data (e.g. from themes.json remote content).
///
/// This is the primary way to load themes. The themes.json file in app-files
/// is the source of truth for all platform themes. Core bundles a single
/// fallback theme for first-launch; all others come from this JSON.
pub fn load_themes_from_json(data: &[u8]) -> Result<Vec<Theme>, ThemeError> {
    serde_json::from_slice(data).map_err(|e| ThemeError::InvalidThemeJson(e.to_string()))
}

/// Get the single bundled default theme (fallback for first-launch before
/// any remote themes are downloaded).
pub fn default_theme() -> Theme {
    default_dark()
}

fn default_dark() -> Theme {
    Theme {
        id: "catppuccin-mocha".to_string(),
        name: "Catppuccin Mocha".to_string(),
        version: "1.0.0".to_string(),
        author: Some("Catppuccin".to_string()),
        license: Some("MIT".to_string()),
        source: Some("https://github.com/catppuccin/catppuccin".to_string()),
        mode: ThemeMode::Dark,
        colors: ThemeColors {
            bg_primary: "#1e1e2e".to_string(),
            bg_secondary: "#181825".to_string(),
            bg_tertiary: "#313244".to_string(),
            text_primary: "#cdd6f4".to_string(),
            text_secondary: "#a6adc8".to_string(),
            accent: "#89b4fa".to_string(),
            accent_dark: "#74c7ec".to_string(),
            success: "#a6e3a1".to_string(),
            error: "#f38ba8".to_string(),
            warning: "#fab387".to_string(),
            border: "#45475a".to_string(),
        },
        tokens: DesignTokens::default(),
    }
}

// INLINE_TEST_REQUIRED: contract test needs access to load_themes_from_json and internal parsing
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_valid() {
        parse_hex("#ffffff").expect("expected success");
        assert_eq!(parse_hex("#ffffff").unwrap(), (255, 255, 255));
        assert_eq!(parse_hex("#000000").unwrap(), (0, 0, 0));
        assert_eq!(parse_hex("#1e1e2e").unwrap(), (30, 30, 46));
    }

    #[test]
    fn test_parse_hex_invalid() {
        parse_hex("ffffff").expect_err("expected error");
        parse_hex("#fff").expect_err("expected error");
        parse_hex("#gggggg").expect_err("expected error");
    }

    #[test]
    fn test_contrast_ratio_black_white() {
        let ratio = contrast_ratio((255, 255, 255), (0, 0, 0));
        assert!(ratio > 20.0, "White on black should have high contrast");
    }

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
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../themes/generated/themes.json");
        let data = std::fs::read(&path).ok()?;
        Some(load_themes_from_json(&data).expect("generated/themes.json must be valid"))
    }

    #[test]
    fn test_themes_json_not_empty() {
        let Some(themes) = load_generated_themes() else {
            eprintln!("SKIP: themes/generated/themes.json not found");
            return;
        };
        assert!(!themes.is_empty());
    }

    #[test]
    fn test_theme_by_id_found_in_json() {
        let Some(themes) = load_generated_themes() else {
            return;
        };
        let found = themes.iter().find(|t| t.id == "catppuccin-mocha");
        found.expect("expected Some");
    }

    #[test]
    fn test_theme_by_id_not_found_in_json() {
        let Some(themes) = load_generated_themes() else {
            return;
        };
        let found = themes.iter().find(|t| t.id == "nonexistent");
        assert!(found.is_none());
    }

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

    #[test]
    fn test_load_themes_from_json_invalid_json() {
        let result = load_themes_from_json(b"not json");
        result.expect_err("expected error");
    }

    #[test]
    fn test_load_themes_from_json_empty_array() {
        let result = load_themes_from_json(b"[]").unwrap();
        assert!(result.is_empty());
    }

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

    #[test]
    fn test_design_tokens_default_spacing() {
        let tokens = DesignTokens::default();
        assert_eq!(tokens.spacing.xs, 4);
        assert_eq!(tokens.spacing.sm, 8);
        assert_eq!(tokens.spacing.md, 16);
        assert_eq!(tokens.spacing.lg, 24);
        assert_eq!(tokens.spacing.xl, 32);
    }

    #[test]
    fn test_design_tokens_default_typography() {
        let tokens = DesignTokens::default();
        assert_eq!(tokens.typography.title_size, 24);
        assert_eq!(tokens.typography.subtitle_size, 18);
        assert_eq!(tokens.typography.body_size, 16);
        assert_eq!(tokens.typography.caption_size, 14);
    }

    #[test]
    fn test_design_tokens_default_border_radius() {
        let tokens = DesignTokens::default();
        assert_eq!(tokens.border_radius.sm, 4);
        assert_eq!(tokens.border_radius.md, 8);
        assert_eq!(tokens.border_radius.lg, 16);
    }

    #[test]
    fn test_design_tokens_serde_roundtrip() {
        let tokens = DesignTokens::default();
        let json = serde_json::to_string(&tokens).unwrap();
        let restored: DesignTokens = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, tokens);
    }

    #[test]
    fn test_theme_includes_default_tokens() {
        let theme = default_theme();
        assert_eq!(theme.tokens.spacing.md, 16);
        assert_eq!(theme.tokens.typography.body_size, 16);
        assert_eq!(theme.tokens.border_radius.md, 8);
    }

    #[test]
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
}
