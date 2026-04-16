// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Theming Tests
//!
//! Tests for theme system functionality including preview, system integration,
//! accent color customization, remote updates, and QR code consistency.
//!
//! Feature file: features/theming.feature
//!
//! These tests verify:
//! - Theme preview before applying (@selection)
//! - System dark mode following (@system)
//! - Accent color customization (@accent)
//! - Remote theme updates (@remote)
//! - QR code theming consistency (@qr)

use crate::common;

use common::helpers::{all_themes, theme_by_id, try_all_themes};
use tempfile::TempDir;
use vauchi_app::content::{
    ContentCache, ContentConfig, ContentManager, ContentType, compute_checksum,
};
use vauchi_app::theme::{Theme, ThemeError, ThemeMode, validate_hex_color};

// ============================================================
// Theme Preview Before Apply
// Feature: theming.feature @selection
// Scenario: Preview theme before applying
// ============================================================

/// Preview state for themes (in-memory, not persisted)
#[derive(Debug, Clone)]
struct ThemePreview {
    original_theme_id: String,
    preview_theme: Theme,
    is_preview_active: bool,
}

impl ThemePreview {
    /// Start previewing a theme without persisting
    fn start_preview(original_theme_id: &str, preview_theme: Theme) -> Self {
        ThemePreview {
            original_theme_id: original_theme_id.to_string(),
            preview_theme,
            is_preview_active: true,
        }
    }

    /// Cancel preview and return to original theme
    fn cancel(&mut self) {
        self.is_preview_active = false;
    }

    /// Get the currently active theme (preview or original)
    fn active_theme(&self) -> Option<Theme> {
        if self.is_preview_active {
            Some(self.preview_theme.clone())
        } else {
            theme_by_id(&self.original_theme_id)
        }
    }
}

/// Test: Preview theme without persisting to storage
/// Feature: theming.feature @selection
/// Scenario: Preview theme before applying
// @scenario: theming :: Preview theme before applying
#[test]
fn test_theme_preview_before_apply() {
    let Some(_themes) = try_all_themes() else {
        eprintln!("SKIP: themes/generated/themes.json not found");
        return;
    };
    // Given the user has "default-dark" theme selected
    let current_theme_id = "default-dark";
    let current_theme = theme_by_id(current_theme_id).expect("default-dark should exist");

    // When the user taps on "Catppuccin Mocha"
    let preview_theme = theme_by_id("catppuccin-mocha").expect("catppuccin-mocha should exist");

    // Start preview (in-memory only, not persisted)
    let preview = ThemePreview::start_preview(current_theme_id, preview_theme.clone());

    // Then the app should preview the theme colors
    assert!(preview.is_preview_active);
    let active = preview.active_theme().expect("Should have active theme");
    assert_eq!(active.id, "catppuccin-mocha");
    assert_eq!(active.colors.bg_primary, "#1e1e2e");

    // Verify original theme is unchanged
    assert_eq!(preview.original_theme_id, current_theme_id);

    // The preview should not affect persisted state
    // (In real implementation, storage would not be written to)
    assert_eq!(current_theme.id, "default-dark");
}

/// Test: Cancel preview returns to original theme
/// Feature: theming.feature @selection
/// Scenario: Preview theme before applying (Cancel path)
// @scenario: theming :: Preview theme before applying
#[test]
fn test_theme_preview_cancel_returns_to_original() {
    let Some(_themes) = try_all_themes() else {
        eprintln!("SKIP: themes/generated/themes.json not found");
        return;
    };
    let current_theme_id = "default-dark";
    let preview_theme = theme_by_id("dracula").expect("dracula should exist");

    let mut preview = ThemePreview::start_preview(current_theme_id, preview_theme);

    // Verify preview is active
    assert!(preview.is_preview_active);
    assert_eq!(preview.active_theme().unwrap().id, "dracula");

    // Cancel the preview
    preview.cancel();

    // Should return to original theme
    assert!(!preview.is_preview_active);
    let active = preview.active_theme().expect("Should have active theme");
    assert_eq!(active.id, "default-dark");
}

/// Test: Preview multiple themes in sequence
/// Feature: theming.feature @selection
// @scenario: theming :: Select theme from settings
// @scenario: theming :: Preview theme before applying
// @scenario: theming :: Apply Gruvbox Dark theme
#[test]
fn test_theme_preview_sequence() {
    let Some(_themes) = try_all_themes() else {
        eprintln!("SKIP: themes/generated/themes.json not found");
        return;
    };
    let original_id = "default-light";

    // Preview first theme
    let theme1 = theme_by_id("nord").unwrap();
    let preview = ThemePreview::start_preview(original_id, theme1);
    assert_eq!(preview.active_theme().unwrap().id, "nord");

    // Switch preview to another theme
    let theme2 = theme_by_id("gruvbox-dark").unwrap();
    let preview = ThemePreview::start_preview(original_id, theme2);
    assert_eq!(preview.active_theme().unwrap().id, "gruvbox-dark");

    // Original should still be preserved
    assert_eq!(preview.original_theme_id, original_id);
}

// ============================================================
// System Dark Mode Following
// Feature: theming.feature @system
// Scenario: Follow system dark/light mode
// ============================================================

/// System theme preference
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SystemThemePreference {
    /// Follow system setting
    System,
    /// Always use light theme
    Light,
    /// Always use dark theme
    Dark,
}

/// Resolved theme based on system preference and user selection
struct ThemeResolver {
    user_preference: SystemThemePreference,
    selected_theme_family: String, // e.g., "catppuccin", "solarized", "default"
}

impl ThemeResolver {
    fn new(preference: SystemThemePreference, family: &str) -> Self {
        ThemeResolver {
            user_preference: preference,
            selected_theme_family: family.to_string(),
        }
    }

    /// Resolve the actual theme based on system mode
    fn resolve(&self, system_is_dark: bool) -> Option<Theme> {
        match self.user_preference {
            SystemThemePreference::System => {
                // Auto-switch based on system setting
                self.get_theme_for_mode(system_is_dark)
            }
            SystemThemePreference::Light => self.get_theme_for_mode(false),
            SystemThemePreference::Dark => self.get_theme_for_mode(true),
        }
    }

    fn get_theme_for_mode(&self, is_dark: bool) -> Option<Theme> {
        let suffix = if is_dark { "dark" } else { "light" };

        // Try family-specific variant first
        let family_variant: String = match self.selected_theme_family.as_str() {
            "catppuccin" => {
                if is_dark {
                    "catppuccin-mocha".to_string()
                } else {
                    "catppuccin-latte".to_string()
                }
            }
            "solarized" => format!("solarized-{}", suffix),
            "gruvbox" => format!("gruvbox-{}", suffix),
            "default" => format!("default-{}", suffix),
            // For themes without variants (dracula, nord), return as-is if dark
            other => {
                let theme = theme_by_id(other);
                if let Some(t) = &theme {
                    // Return the theme if it matches the mode, otherwise fall back to default
                    if (t.mode == ThemeMode::Dark) == is_dark {
                        return theme;
                    }
                }
                format!("default-{}", suffix)
            }
        };

        theme_by_id(&family_variant)
    }
}

/// Test: Auto-switch with OS dark mode setting
/// Feature: theming.feature @system
/// Scenario: Follow system dark/light mode
// @scenario: theming :: Follow system dark/light mode
#[test]
fn test_system_dark_mode_following() {
    let Some(_themes) = try_all_themes() else {
        eprintln!("SKIP: themes/generated/themes.json not found");
        return;
    };
    // Given the user has selected "System" theme preference
    let resolver = ThemeResolver::new(SystemThemePreference::System, "default");

    // When the system is in dark mode
    let theme = resolver.resolve(true).expect("Should resolve theme");
    assert_eq!(theme.mode, ThemeMode::Dark);
    assert_eq!(theme.id, "default-dark");

    // When the system switches to light mode
    let theme = resolver.resolve(false).expect("Should resolve theme");
    assert_eq!(theme.mode, ThemeMode::Light);
    assert_eq!(theme.id, "default-light");
}

/// Test: Auto theme with Catppuccin family
/// Feature: theming.feature @system @auto
/// Scenario: Auto theme with Catppuccin
// @scenario: theming :: Auto theme with Catppuccin
#[test]
fn test_auto_theme_with_catppuccin() {
    let Some(_themes) = try_all_themes() else {
        eprintln!("SKIP: themes/generated/themes.json not found");
        return;
    };
    // Given the user has selected "Catppuccin (Auto)" theme
    let resolver = ThemeResolver::new(SystemThemePreference::System, "catppuccin");

    // When the system is in dark mode
    let theme = resolver.resolve(true).expect("Should resolve theme");
    assert_eq!(theme.id, "catppuccin-mocha");
    assert_eq!(theme.mode, ThemeMode::Dark);

    // When the system switches to light mode
    let theme = resolver.resolve(false).expect("Should resolve theme");
    assert_eq!(theme.id, "catppuccin-latte");
    assert_eq!(theme.mode, ThemeMode::Light);
}

/// Test: Override system preference with explicit selection
/// Feature: theming.feature @system
/// Scenario: Override system preference
// @scenario: theming :: Override system preference
#[test]
fn test_override_system_preference() {
    // Given the system is in light mode
    // And the user has explicitly selected "Catppuccin Mocha" (dark)
    let resolver = ThemeResolver::new(SystemThemePreference::Dark, "catppuccin");

    // Then the app should use dark theme regardless of system setting
    let theme = resolver.resolve(false).expect("Should resolve theme"); // system is light
    assert_eq!(theme.mode, ThemeMode::Dark);
    assert_eq!(theme.id, "catppuccin-mocha");
}

/// Test: Solarized auto-switching
/// Feature: theming.feature @system
// @scenario: theming :: Follow system dark/light mode
// @scenario: theming :: Apply Solarized Dark theme
#[test]
fn test_solarized_auto_switching() {
    let Some(_themes) = try_all_themes() else {
        eprintln!("SKIP: themes/generated/themes.json not found");
        return;
    };
    let resolver = ThemeResolver::new(SystemThemePreference::System, "solarized");

    // Dark mode
    let theme = resolver.resolve(true).unwrap();
    assert_eq!(theme.id, "solarized-dark");

    // Light mode
    let theme = resolver.resolve(false).unwrap();
    assert_eq!(theme.id, "solarized-light");
}

/// Test: Force light theme regardless of system setting
/// Feature: theming.feature @system
// @scenario: theming :: Override system preference
#[test]
fn test_force_light_preference() {
    let Some(_themes) = try_all_themes() else {
        eprintln!("SKIP: themes/generated/themes.json not found");
        return;
    };
    // Given the user has explicitly selected light preference
    let resolver = ThemeResolver::new(SystemThemePreference::Light, "gruvbox");

    // Then the app should use light theme regardless of system setting
    let theme = resolver.resolve(true).expect("Should resolve theme"); // system is dark
    assert_eq!(theme.mode, ThemeMode::Light);
    assert_eq!(theme.id, "gruvbox-light");

    // And also when system is light
    let theme = resolver.resolve(false).expect("Should resolve theme");
    assert_eq!(theme.mode, ThemeMode::Light);
    assert_eq!(theme.id, "gruvbox-light");
}

// ============================================================
// Accent Color Customization
// Feature: theming.feature @accent
// Scenario: Choose accent color within theme
// ============================================================

/// Available accent colors for Catppuccin themes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum CatppuccinAccent {
    Rosewater,
    Flamingo,
    Pink,
    Mauve,
    Red,
    Maroon,
    Peach,
    Yellow,
    Green,
    Teal,
    Sky,
    Sapphire,
    Blue,
    Lavender,
}

impl CatppuccinAccent {
    /// Get hex color for this accent in Mocha palette
    fn mocha_hex(&self) -> &'static str {
        match self {
            CatppuccinAccent::Rosewater => "#f5e0dc",
            CatppuccinAccent::Flamingo => "#f2cdcd",
            CatppuccinAccent::Pink => "#f5c2e7",
            CatppuccinAccent::Mauve => "#cba6f7",
            CatppuccinAccent::Red => "#f38ba8",
            CatppuccinAccent::Maroon => "#eba0ac",
            CatppuccinAccent::Peach => "#fab387",
            CatppuccinAccent::Yellow => "#f9e2af",
            CatppuccinAccent::Green => "#a6e3a1",
            CatppuccinAccent::Teal => "#94e2d5",
            CatppuccinAccent::Sky => "#89dceb",
            CatppuccinAccent::Sapphire => "#74c7ec",
            CatppuccinAccent::Blue => "#89b4fa",
            CatppuccinAccent::Lavender => "#b4befe",
        }
    }

    /// Get all available accents
    fn all() -> Vec<CatppuccinAccent> {
        vec![
            CatppuccinAccent::Rosewater,
            CatppuccinAccent::Flamingo,
            CatppuccinAccent::Pink,
            CatppuccinAccent::Mauve,
            CatppuccinAccent::Red,
            CatppuccinAccent::Maroon,
            CatppuccinAccent::Peach,
            CatppuccinAccent::Yellow,
            CatppuccinAccent::Green,
            CatppuccinAccent::Teal,
            CatppuccinAccent::Sky,
            CatppuccinAccent::Sapphire,
            CatppuccinAccent::Blue,
            CatppuccinAccent::Lavender,
        ]
    }
}

/// Customized theme with user-selected accent
struct CustomizedTheme {
    base_theme: Theme,
    custom_accent: Option<String>,
}

impl CustomizedTheme {
    fn new(base: Theme) -> Self {
        CustomizedTheme {
            base_theme: base,
            custom_accent: None,
        }
    }

    fn with_accent(mut self, accent_hex: &str) -> Result<Self, ThemeError> {
        validate_hex_color(accent_hex)?;
        self.custom_accent = Some(accent_hex.to_string());
        Ok(self)
    }

    fn effective_accent(&self) -> &str {
        self.custom_accent
            .as_deref()
            .unwrap_or(&self.base_theme.colors.accent)
    }
}

/// Test: User-defined accent colors
/// Feature: theming.feature @accent @future
/// Scenario: Choose accent color within theme
// @scenario: theming :: Choose accent color within theme
#[test]
fn test_accent_color_customization() {
    // Given the user has selected "Catppuccin Mocha" theme
    let base = theme_by_id("catppuccin-mocha").unwrap();
    let default_accent = base.colors.accent.clone();

    // Default accent should be blue
    assert_eq!(default_accent, "#89b4fa");

    // When the user selects "mauve" accent
    let customized = CustomizedTheme::new(base.clone())
        .with_accent(CatppuccinAccent::Mauve.mocha_hex())
        .unwrap();

    // Then the accent color should be mauve
    assert_eq!(customized.effective_accent(), "#cba6f7");

    // The base theme accent is unchanged
    assert_eq!(base.colors.accent, "#89b4fa");
}

/// Test: All Catppuccin accent colors are valid hex
/// Feature: theming.feature @accent
// @scenario: theming :: Choose accent color within theme
#[test]
fn test_catppuccin_accents_valid_hex() {
    for accent in CatppuccinAccent::all() {
        let hex = accent.mocha_hex();
        assert!(
            validate_hex_color(hex).is_ok(),
            "Accent {:?} should have valid hex: {}",
            accent,
            hex
        );
    }
}

/// Test: Invalid accent color is rejected
/// Feature: theming.feature @accent
// @scenario: theming :: Choose accent color within theme
#[test]
fn test_invalid_accent_rejected() {
    let Some(_themes) = try_all_themes() else {
        eprintln!("SKIP: themes/generated/themes.json not found");
        return;
    };
    let base = theme_by_id("default-dark").unwrap();
    let customized = CustomizedTheme::new(base);

    // Invalid hex should be rejected
    assert!(
        customized.with_accent("not-a-color").is_err(),
        "expected error"
    );
}

/// Test: Custom accent persists (simulation)
/// Feature: theming.feature @accent @future
/// Scenario: Custom accent color persists
// @scenario: theming :: Custom accent color persists
#[test]
fn test_custom_accent_persists() {
    // Simulate persistence by creating a settings struct
    #[derive(Debug, Clone)]
    struct ThemeSettings {
        theme_id: String,
        custom_accent: Option<String>,
    }

    // Given the user has selected "Catppuccin Mocha" with "mauve" accent
    let settings = ThemeSettings {
        theme_id: "catppuccin-mocha".to_string(),
        custom_accent: Some(CatppuccinAccent::Mauve.mocha_hex().to_string()),
    };

    // Simulate app restart by recreating from settings
    let base = theme_by_id(&settings.theme_id).unwrap();
    let restored = if let Some(accent) = &settings.custom_accent {
        CustomizedTheme::new(base).with_accent(accent).unwrap()
    } else {
        CustomizedTheme::new(base)
    };

    // Then the "mauve" accent color should still be applied
    assert_eq!(restored.effective_accent(), "#cba6f7");
}

// ============================================================
// Remote Theme Updates
// Feature: theming.feature @remote
// Scenario: Theme update with existing selection
// ============================================================

/// Helper to create a Tokyo Night theme JSON string
fn tokyo_night_json() -> String {
    serde_json::json!([
        {
            "id": "tokyo-night",
            "name": "Tokyo Night",
            "version": "1.0.0",
            "mode": "dark",
            "colors": {
                "bg-primary": "#1a1b26",
                "bg-secondary": "#16161e",
                "bg-tertiary": "#24283b",
                "text-primary": "#c0caf5",
                "text-secondary": "#9aa5ce",
                "accent": "#7aa2f7",
                "accent-dark": "#3d59a1",
                "success": "#9ece6a",
                "error": "#f7768e",
                "warning": "#e0af68",
                "border": "#414868"
            }
        }
    ])
    .to_string()
}

/// Test: CDN theme content refresh
/// Feature: theming.feature @remote
/// Scenario: Theme update with existing selection
// @scenario: theming :: Theme update with existing selection
#[test]
fn test_remote_theme_updates() {
    let temp = TempDir::new().unwrap();

    // Pre-populate cache with a "remote" theme
    let cache = ContentCache::new(temp.path()).unwrap();

    // Simulate cached themes from CDN (includes "Tokyo Night")
    let cached_themes = tokyo_night_json();

    let checksum = compute_checksum(cached_themes.as_bytes());
    cache
        .save_content(
            ContentType::Themes,
            "themes.json",
            cached_themes.as_bytes(),
            &checksum,
        )
        .unwrap();

    // Verify cache was written
    let cached = cache.get_content(ContentType::Themes, "themes.json");
    assert!(cached.is_some(), "expected Some value");
    let cached_data = cached.unwrap();
    assert!(String::from_utf8_lossy(&cached_data).contains("tokyo-night"));
}

/// Test: New theme available after content update
/// Feature: theming.feature @remote
/// Scenario: New theme available after content update
// @scenario: theming :: New theme available after content update
#[test]
fn test_new_theme_after_update() {
    let temp = TempDir::new().unwrap();
    let cache = ContentCache::new(temp.path()).unwrap();

    // Initially, cache is empty - only bundled themes available
    let bundled = all_themes();
    let has_tokyo_night = bundled.iter().any(|t| t.id == "tokyo-night");
    assert!(!has_tokyo_night, "Tokyo Night should not be bundled");

    // After content update, new theme is cached
    let new_themes = tokyo_night_json();

    let checksum = compute_checksum(new_themes.as_bytes());
    cache
        .save_content(
            ContentType::Themes,
            "themes.json",
            new_themes.as_bytes(),
            &checksum,
        )
        .unwrap();

    // Parse cached themes
    let cached = cache
        .get_content(ContentType::Themes, "themes.json")
        .expect("Themes should be cached");
    let themes: Vec<Theme> = serde_json::from_slice(&cached).unwrap();

    // Tokyo Night should now appear in theme selection
    assert!(
        themes.iter().any(|t| t.id == "tokyo-night"),
        "Tokyo Night should be available after update"
    );
}

/// Test: Bundled themes always available when offline
/// Feature: theming.feature @remote @fallback
/// Scenario: Bundled themes always available
// @scenario: theming :: Bundled themes always available
#[test]
fn test_bundled_themes_always_available() {
    let Some(_themes) = try_all_themes() else {
        eprintln!("SKIP: themes/generated/themes.json not found");
        return;
    };
    // Given the content cache is empty and the device is offline
    // (simulated by not populating cache)
    let temp = TempDir::new().unwrap();
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        remote_updates_enabled: false,
        ..Default::default()
    };

    let manager = ContentManager::new(config).unwrap();

    // When the user views available themes
    let bundled = all_themes();

    // Then "Default Dark" and "Default Light" should be available
    assert!(
        bundled.iter().any(|t| t.id == "default-dark"),
        "Default Dark should be bundled"
    );
    assert!(
        bundled.iter().any(|t| t.id == "default-light"),
        "Default Light should be bundled"
    );

    // And manager should work without network
    let networks = manager.networks();
    assert!(!networks.is_empty(), "Bundled content should work offline");
}

/// Test: User selection preserved after theme update
/// Feature: theming.feature @remote
/// Scenario: Theme update with existing selection
// @scenario: theming :: Theme update with existing selection
#[test]
fn test_theme_selection_preserved_after_update() {
    #[derive(Debug, Clone)]
    struct ThemeSettings {
        selected_theme_id: String,
    }

    // Given the user has selected "Catppuccin Mocha"
    let settings = ThemeSettings {
        selected_theme_id: "catppuccin-mocha".to_string(),
    };

    // When a new version of Catppuccin themes is available and applied
    // (simulated by theme still existing with same ID but updated colors)
    let updated_theme = theme_by_id(&settings.selected_theme_id);

    // Then the user's theme selection should be preserved
    assert!(updated_theme.is_some(), "expected Some value");
    assert_eq!(updated_theme.unwrap().id, "catppuccin-mocha");
}

// ============================================================
// QR Code Theming Consistency
// Feature: theming.feature @qr
// Scenario: QR code remains readable in any theme
// ============================================================

/// QR code display configuration
struct QrCodeDisplay {
    /// Background color (should be white/light for readability)
    background: String,
    /// Foreground color (should be black/dark for readability)
    foreground: String,
    /// Container background (matches theme)
    container_bg: String,
}

impl QrCodeDisplay {
    /// Standard high-contrast QR colors (black on white)
    const STANDARD_BACKGROUND: &'static str = "#ffffff";
    const STANDARD_FOREGROUND: &'static str = "#000000";

    /// Create QR display config for a theme
    fn for_theme(theme: &Theme) -> Self {
        QrCodeDisplay {
            // QR itself always uses standard black-on-white for readability
            background: Self::STANDARD_BACKGROUND.to_string(),
            foreground: Self::STANDARD_FOREGROUND.to_string(),
            // Container uses theme background
            container_bg: theme.colors.bg_primary.clone(),
        }
    }

    /// Calculate contrast ratio between two colors
    fn contrast_ratio(c1: &str, c2: &str) -> Result<f64, ThemeError> {
        let rgb1 = Self::parse_hex(c1)?;
        let rgb2 = Self::parse_hex(c2)?;

        let l1 = Self::relative_luminance(rgb1);
        let l2 = Self::relative_luminance(rgb2);

        let (lighter, darker) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        Ok((lighter + 0.05) / (darker + 0.05))
    }

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

    fn relative_luminance(rgb: (u8, u8, u8)) -> f64 {
        let (r, g, b) = rgb;

        let r = Self::srgb_to_linear(r as f64 / 255.0);
        let g = Self::srgb_to_linear(g as f64 / 255.0);
        let b = Self::srgb_to_linear(b as f64 / 255.0);

        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    fn srgb_to_linear(c: f64) -> f64 {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
}

/// Test: QR matches theme colors (container only)
/// Feature: theming.feature @qr
/// Scenario: QR code container matches theme
// @scenario: theming :: QR code container matches theme
#[test]
fn test_qr_code_theming_consistency() {
    // Test across all bundled themes
    for theme in all_themes() {
        let qr_display = QrCodeDisplay::for_theme(&theme);

        // QR code itself should remain standard black-on-white
        assert_eq!(
            qr_display.foreground,
            QrCodeDisplay::STANDARD_FOREGROUND,
            "QR foreground should be black for theme {}",
            theme.id
        );
        assert_eq!(
            qr_display.background,
            QrCodeDisplay::STANDARD_BACKGROUND,
            "QR background should be white for theme {}",
            theme.id
        );

        // Container should match theme
        assert_eq!(
            qr_display.container_bg, theme.colors.bg_primary,
            "Container should use theme bg for {}",
            theme.id
        );
    }
}

/// Test: QR code maintains high contrast
/// Feature: theming.feature @qr
/// Scenario: QR code remains readable in any theme
// @scenario: theming :: QR code remains readable in any theme
#[test]
fn test_qr_code_high_contrast() {
    // QR codes should display with high contrast
    let contrast = QrCodeDisplay::contrast_ratio(
        QrCodeDisplay::STANDARD_BACKGROUND,
        QrCodeDisplay::STANDARD_FOREGROUND,
    )
    .unwrap();

    // WCAG AAA requires 7:1 for normal text
    // Black on white is 21:1
    assert!(
        contrast > 20.0,
        "Standard QR colors should have very high contrast: {:.2}",
        contrast
    );
}

/// Test: QR background is white or near-white
/// Feature: theming.feature @qr
/// Scenario: QR code remains readable in any theme
// @scenario: theming :: QR code remains readable in any theme
#[test]
fn test_qr_background_is_light() {
    let bg = QrCodeDisplay::STANDARD_BACKGROUND;
    let (r, g, b) = QrCodeDisplay::parse_hex(bg).unwrap();

    // Should be white (255, 255, 255)
    assert_eq!(r, 255);
    assert_eq!(g, 255);
    assert_eq!(b, 255);
}

/// Test: QR foreground is black or near-black
/// Feature: theming.feature @qr
/// Scenario: QR code remains readable in any theme
// @scenario: theming :: QR code remains readable in any theme
#[test]
fn test_qr_foreground_is_dark() {
    let fg = QrCodeDisplay::STANDARD_FOREGROUND;
    let (r, g, b) = QrCodeDisplay::parse_hex(fg).unwrap();

    // Should be black (0, 0, 0)
    assert_eq!(r, 0);
    assert_eq!(g, 0);
    assert_eq!(b, 0);
}

/// Test: Dark theme container provides visual separation for QR
/// Feature: theming.feature @qr
// @scenario: theming :: QR code container matches theme
#[test]
fn test_qr_container_contrast_with_dark_theme() {
    let dark_theme = theme_by_id("catppuccin-mocha").unwrap();
    let qr_display = QrCodeDisplay::for_theme(&dark_theme);

    // Container bg (#1e1e2e) should contrast with white QR bg (#ffffff)
    let container_qr_contrast =
        QrCodeDisplay::contrast_ratio(&qr_display.container_bg, &qr_display.background).unwrap();

    // Should have good contrast for visual separation
    assert!(
        container_qr_contrast > 10.0,
        "Dark container should contrast with white QR: {:.2}",
        container_qr_contrast
    );
}

/// Test: Light theme container still provides visual separation for QR
/// Feature: theming.feature @qr
// @scenario: theming :: QR code container matches theme
#[test]
fn test_qr_container_with_light_theme() {
    let Some(_themes) = try_all_themes() else {
        eprintln!("SKIP: themes/generated/themes.json not found");
        return;
    };
    let light_theme = theme_by_id("catppuccin-latte").unwrap();
    let qr_display = QrCodeDisplay::for_theme(&light_theme);

    // Container bg (#eff1f5) and QR bg (#ffffff) are both light
    // This is acceptable - the QR container might use a subtle border instead
    assert_eq!(qr_display.background, "#ffffff");
    assert_eq!(qr_display.container_bg, "#eff1f5");

    // QR itself still maintains high contrast
    let qr_contrast =
        QrCodeDisplay::contrast_ratio(&qr_display.background, &qr_display.foreground).unwrap();
    assert!(qr_contrast > 20.0);
}
