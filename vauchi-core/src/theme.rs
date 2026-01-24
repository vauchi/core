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

/// Get all bundled themes
pub fn get_bundled_themes() -> Vec<Theme> {
    vec![
        // Default themes
        default_dark(),
        default_light(),
        // Catppuccin
        catppuccin_mocha(),
        catppuccin_latte(),
        catppuccin_frappe(),
        catppuccin_macchiato(),
        // Popular themes
        dracula(),
        nord(),
        solarized_dark(),
        solarized_light(),
        gruvbox_dark(),
        gruvbox_light(),
    ]
}

/// Get a theme by ID
pub fn get_theme_by_id(id: &str) -> Option<Theme> {
    get_bundled_themes().into_iter().find(|t| t.id == id)
}

// ============================================================
// Default Themes
// ============================================================

fn default_dark() -> Theme {
    Theme {
        id: "default-dark".to_string(),
        name: "Vauchi Dark".to_string(),
        version: "1.0.0".to_string(),
        author: Some("Vauchi".to_string()),
        license: Some("MIT".to_string()),
        source: None,
        mode: ThemeMode::Dark,
        colors: ThemeColors {
            bg_primary: "#1a1a2e".to_string(),
            bg_secondary: "#16213e".to_string(),
            bg_tertiary: "#0f3460".to_string(),
            text_primary: "#eeeeee".to_string(),
            text_secondary: "#a0a0a0".to_string(),
            accent: "#4fc3f7".to_string(),
            accent_dark: "#0288d1".to_string(),
            success: "#4caf50".to_string(),
            error: "#f44336".to_string(),
            warning: "#ff9800".to_string(),
            border: "#333333".to_string(),
        },
    }
}

fn default_light() -> Theme {
    Theme {
        id: "default-light".to_string(),
        name: "Vauchi Light".to_string(),
        version: "1.0.0".to_string(),
        author: Some("Vauchi".to_string()),
        license: Some("MIT".to_string()),
        source: None,
        mode: ThemeMode::Light,
        colors: ThemeColors {
            bg_primary: "#ffffff".to_string(),
            bg_secondary: "#f5f5f5".to_string(),
            bg_tertiary: "#e0e0e0".to_string(),
            text_primary: "#212121".to_string(),
            text_secondary: "#757575".to_string(),
            accent: "#1976d2".to_string(),
            accent_dark: "#0d47a1".to_string(),
            success: "#388e3c".to_string(),
            error: "#d32f2f".to_string(),
            warning: "#f57c00".to_string(),
            border: "#e0e0e0".to_string(),
        },
    }
}

// ============================================================
// Catppuccin Themes
// https://github.com/catppuccin/catppuccin
// ============================================================

fn catppuccin_mocha() -> Theme {
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
    }
}

fn catppuccin_latte() -> Theme {
    Theme {
        id: "catppuccin-latte".to_string(),
        name: "Catppuccin Latte".to_string(),
        version: "1.0.0".to_string(),
        author: Some("Catppuccin".to_string()),
        license: Some("MIT".to_string()),
        source: Some("https://github.com/catppuccin/catppuccin".to_string()),
        mode: ThemeMode::Light,
        colors: ThemeColors {
            bg_primary: "#eff1f5".to_string(),
            bg_secondary: "#e6e9ef".to_string(),
            bg_tertiary: "#ccd0da".to_string(),
            text_primary: "#4c4f69".to_string(),
            text_secondary: "#6c6f85".to_string(),
            accent: "#1e66f5".to_string(),
            accent_dark: "#209fb5".to_string(),
            success: "#40a02b".to_string(),
            error: "#d20f39".to_string(),
            warning: "#fe640b".to_string(),
            border: "#9ca0b0".to_string(),
        },
    }
}

fn catppuccin_frappe() -> Theme {
    Theme {
        id: "catppuccin-frappe".to_string(),
        name: "Catppuccin Frappé".to_string(),
        version: "1.0.0".to_string(),
        author: Some("Catppuccin".to_string()),
        license: Some("MIT".to_string()),
        source: Some("https://github.com/catppuccin/catppuccin".to_string()),
        mode: ThemeMode::Dark,
        colors: ThemeColors {
            bg_primary: "#303446".to_string(),
            bg_secondary: "#292c3c".to_string(),
            bg_tertiary: "#414559".to_string(),
            text_primary: "#c6d0f5".to_string(),
            text_secondary: "#a5adce".to_string(),
            accent: "#8caaee".to_string(),
            accent_dark: "#85c1dc".to_string(),
            success: "#a6d189".to_string(),
            error: "#e78284".to_string(),
            warning: "#ef9f76".to_string(),
            border: "#51576d".to_string(),
        },
    }
}

fn catppuccin_macchiato() -> Theme {
    Theme {
        id: "catppuccin-macchiato".to_string(),
        name: "Catppuccin Macchiato".to_string(),
        version: "1.0.0".to_string(),
        author: Some("Catppuccin".to_string()),
        license: Some("MIT".to_string()),
        source: Some("https://github.com/catppuccin/catppuccin".to_string()),
        mode: ThemeMode::Dark,
        colors: ThemeColors {
            bg_primary: "#24273a".to_string(),
            bg_secondary: "#1e2030".to_string(),
            bg_tertiary: "#363a4f".to_string(),
            text_primary: "#cad3f5".to_string(),
            text_secondary: "#a5adcb".to_string(),
            accent: "#8aadf4".to_string(),
            accent_dark: "#7dc4e4".to_string(),
            success: "#a6da95".to_string(),
            error: "#ed8796".to_string(),
            warning: "#f5a97f".to_string(),
            border: "#494d64".to_string(),
        },
    }
}

// ============================================================
// Dracula Theme
// https://draculatheme.com
// ============================================================

fn dracula() -> Theme {
    Theme {
        id: "dracula".to_string(),
        name: "Dracula".to_string(),
        version: "1.0.0".to_string(),
        author: Some("Zeno Rocha".to_string()),
        license: Some("MIT".to_string()),
        source: Some("https://draculatheme.com".to_string()),
        mode: ThemeMode::Dark,
        colors: ThemeColors {
            bg_primary: "#282a36".to_string(),
            bg_secondary: "#21222c".to_string(),
            bg_tertiary: "#44475a".to_string(),
            text_primary: "#f8f8f2".to_string(),
            text_secondary: "#6272a4".to_string(),
            accent: "#bd93f9".to_string(),
            accent_dark: "#ff79c6".to_string(),
            success: "#50fa7b".to_string(),
            error: "#ff5555".to_string(),
            warning: "#ffb86c".to_string(),
            border: "#44475a".to_string(),
        },
    }
}

// ============================================================
// Nord Theme
// https://nordtheme.com
// ============================================================

fn nord() -> Theme {
    Theme {
        id: "nord".to_string(),
        name: "Nord".to_string(),
        version: "1.0.0".to_string(),
        author: Some("Arctic Ice Studio".to_string()),
        license: Some("MIT".to_string()),
        source: Some("https://nordtheme.com".to_string()),
        mode: ThemeMode::Dark,
        colors: ThemeColors {
            bg_primary: "#2e3440".to_string(),
            bg_secondary: "#3b4252".to_string(),
            bg_tertiary: "#434c5e".to_string(),
            text_primary: "#eceff4".to_string(),
            text_secondary: "#d8dee9".to_string(),
            accent: "#88c0d0".to_string(),
            accent_dark: "#81a1c1".to_string(),
            success: "#a3be8c".to_string(),
            error: "#bf616a".to_string(),
            warning: "#ebcb8b".to_string(),
            border: "#4c566a".to_string(),
        },
    }
}

// ============================================================
// Solarized Themes
// https://ethanschoonover.com/solarized
// ============================================================

fn solarized_dark() -> Theme {
    Theme {
        id: "solarized-dark".to_string(),
        name: "Solarized Dark".to_string(),
        version: "1.0.0".to_string(),
        author: Some("Ethan Schoonover".to_string()),
        license: Some("MIT".to_string()),
        source: Some("https://ethanschoonover.com/solarized".to_string()),
        mode: ThemeMode::Dark,
        colors: ThemeColors {
            bg_primary: "#002b36".to_string(),
            bg_secondary: "#073642".to_string(),
            bg_tertiary: "#586e75".to_string(),
            text_primary: "#839496".to_string(),
            text_secondary: "#657b83".to_string(),
            accent: "#268bd2".to_string(),
            accent_dark: "#2aa198".to_string(),
            success: "#859900".to_string(),
            error: "#dc322f".to_string(),
            warning: "#b58900".to_string(),
            border: "#073642".to_string(),
        },
    }
}

fn solarized_light() -> Theme {
    Theme {
        id: "solarized-light".to_string(),
        name: "Solarized Light".to_string(),
        version: "1.0.0".to_string(),
        author: Some("Ethan Schoonover".to_string()),
        license: Some("MIT".to_string()),
        source: Some("https://ethanschoonover.com/solarized".to_string()),
        mode: ThemeMode::Light,
        colors: ThemeColors {
            bg_primary: "#fdf6e3".to_string(),
            bg_secondary: "#eee8d5".to_string(),
            bg_tertiary: "#93a1a1".to_string(),
            text_primary: "#586e75".to_string(), // base01 for better contrast
            text_secondary: "#657b83".to_string(),
            accent: "#268bd2".to_string(),
            accent_dark: "#2aa198".to_string(),
            success: "#859900".to_string(),
            error: "#dc322f".to_string(),
            warning: "#b58900".to_string(),
            border: "#eee8d5".to_string(),
        },
    }
}

// ============================================================
// Gruvbox Themes
// https://github.com/morhetz/gruvbox
// ============================================================

fn gruvbox_dark() -> Theme {
    Theme {
        id: "gruvbox-dark".to_string(),
        name: "Gruvbox Dark".to_string(),
        version: "1.0.0".to_string(),
        author: Some("morhetz".to_string()),
        license: Some("MIT".to_string()),
        source: Some("https://github.com/morhetz/gruvbox".to_string()),
        mode: ThemeMode::Dark,
        colors: ThemeColors {
            bg_primary: "#282828".to_string(),
            bg_secondary: "#3c3836".to_string(),
            bg_tertiary: "#504945".to_string(),
            text_primary: "#ebdbb2".to_string(),
            text_secondary: "#a89984".to_string(),
            accent: "#83a598".to_string(),
            accent_dark: "#8ec07c".to_string(),
            success: "#b8bb26".to_string(),
            error: "#fb4934".to_string(),
            warning: "#fabd2f".to_string(),
            border: "#504945".to_string(),
        },
    }
}

fn gruvbox_light() -> Theme {
    Theme {
        id: "gruvbox-light".to_string(),
        name: "Gruvbox Light".to_string(),
        version: "1.0.0".to_string(),
        author: Some("morhetz".to_string()),
        license: Some("MIT".to_string()),
        source: Some("https://github.com/morhetz/gruvbox".to_string()),
        mode: ThemeMode::Light,
        colors: ThemeColors {
            bg_primary: "#fbf1c7".to_string(),
            bg_secondary: "#ebdbb2".to_string(),
            bg_tertiary: "#d5c4a1".to_string(),
            text_primary: "#3c3836".to_string(),
            text_secondary: "#665c54".to_string(),
            accent: "#458588".to_string(),
            accent_dark: "#689d6a".to_string(),
            success: "#98971a".to_string(),
            error: "#cc241d".to_string(),
            warning: "#d79921".to_string(),
            border: "#d5c4a1".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_valid() {
        assert!(parse_hex("#ffffff").is_ok());
        assert_eq!(parse_hex("#ffffff").unwrap(), (255, 255, 255));
        assert_eq!(parse_hex("#000000").unwrap(), (0, 0, 0));
        assert_eq!(parse_hex("#1e1e2e").unwrap(), (30, 30, 46));
    }

    #[test]
    fn test_parse_hex_invalid() {
        assert!(parse_hex("ffffff").is_err());
        assert!(parse_hex("#fff").is_err());
        assert!(parse_hex("#gggggg").is_err());
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

    #[test]
    fn test_bundled_themes_not_empty() {
        let themes = get_bundled_themes();
        assert!(!themes.is_empty());
    }

    #[test]
    fn test_get_theme_by_id_found() {
        let theme = get_theme_by_id("catppuccin-mocha");
        assert!(theme.is_some());
    }

    #[test]
    fn test_get_theme_by_id_not_found() {
        let theme = get_theme_by_id("nonexistent");
        assert!(theme.is_none());
    }
}
