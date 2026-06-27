// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Build script for vauchi-app.
//!
//! Embeds two compile-time bundles into `OUT_DIR`:
//!
//! 1. **`bundled_locale.rs`** — English locale strings from the
//!    sibling `locales/` repo. Provides a complete fallback when
//!    runtime locale files aren't loaded.
//! 2. **`themes.json`** — theme catalog from the sibling `themes/`
//!    repo. Used by `theme::bundled_themes()` to populate
//!    `SettingsConfig.available_themes` (problem record
//!    `2026-05-01-android-humble-ui-deep-retirement` Phase 2a/A3a).
//!
//! The themes bundle deliberately mirrors `vauchi-platform/build.rs` —
//! vauchi-platform depends on vauchi-app (not vice versa), so
//! vauchi-app cannot reuse the platform constant. The 60 KB
//! duplication is intentional; the source-of-truth is the
//! `themes/` repo via the candidate paths.

use std::env;
use std::fs;
use std::path::Path;

/// Frozen byte-identical copy of `themes/tokens.json`, used ONLY as the
/// build-time fallback when the sibling `themes/` repo is absent
/// (crates.io publish, cargo-mutants, out-of-tree). `DesignTokens` has
/// required (non-defaulted) fields, so an empty `{}` cannot parse — the
/// fallback must be a complete, valid token document. Keep in sync with
/// `themes/tokens.json` (the contract checker validates the real one).
const FROZEN_TOKENS_JSON: &str = r#"{
  "_spdx": "SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>; SPDX-License-Identifier: GPL-3.0-or-later",
  "version": "2.0.0",
  "spacing": { "xs": 4, "sm": 8, "sm_md": 12, "md": 16, "lg": 24, "xl": 32 },
  "spacing_direction": { "content_start": 16, "content_end": 16, "list_item_start": 8, "list_item_end": 8, "list_item_inline_start": 12, "list_item_inline_end": 12 },
  "typography": { "title_size": 24, "subtitle_size": 18, "body_size": 16, "caption_size": 14, "caption_sm": 12, "title_lg": 20, "display": 32, "medium_size": 20, "title_line": 30, "subtitle_line": 24, "medium_line": 28, "body_line": 24, "caption_line": 20, "text_scale_percent": 100 },
  "border_radius": { "sm": 4, "md": 8, "md_lg": 12, "lg": 16, "chip": 12, "card": 20, "sheet": 28 },
  "touch_target": { "minimum": 44 },
  "font_family": { "display": "Bricolage Grotesque", "body": "Hanken Grotesk", "mono": "JetBrains Mono" },
  "font_weight": { "regular": 400, "medium": 500, "semibold": 600, "bold": 700, "extrabold": 800 },
  "focus": { "ring_width": 3, "ring_offset": 2 },
  "motion": { "enter_duration_ms": 200, "exit_duration_ms": 150, "emphasis_duration_ms": 300 }
}"#;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("bundled_locale.rs");

    // Check VAUCHI_LOCALES_DIR env var first (for out-of-tree builds like cargo-mutants),
    // then fall back to relative sibling repo paths.
    let env_path = env::var("VAUCHI_LOCALES_DIR")
        .ok()
        .map(|dir| format!("{}/en.json", dir));

    let relative_paths = [
        "../../locales/en.json",    // Standard sibling repo layout
        "../../../locales/en.json", // Alternative layout
        "locales/en.json",          // Local copy (for crates.io publish)
    ];

    let mut locale_content = None;
    let mut found_path = None;

    let all_paths: Vec<String> = env_path
        .into_iter()
        .chain(relative_paths.iter().map(|s| s.to_string()))
        .collect();

    for path in &all_paths {
        if Path::new(path).exists() {
            println!("cargo::rerun-if-changed={}", path);
            if let Ok(content) = fs::read_to_string(path) {
                locale_content = Some(content);
                found_path = Some(path.clone());
                break;
            }
        }
    }

    let generated = if let Some(content) = locale_content {
        eprintln!(
            "cargo:warning=Bundling English locale from: {}",
            found_path.unwrap()
        );

        // Escape for Rust string - convert to bytes representation
        let escaped = content
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");

        format!(
            r#"/// Bundled English locale JSON (embedded at compile time from locales repo)
pub const BUNDLED_EN_JSON: &str = "{}";
"#,
            escaped
        )
    } else {
        eprintln!("cargo:warning=No locale file found, using minimal fallback");

        r#"/// Minimal fallback when locales repo not available
pub const BUNDLED_EN_JSON: &str = "{\"app.name\":\"Vauchi\",\"welcome.title\":\"Welcome to Vauchi\"}";
"#
        .to_string()
    };

    fs::write(&dest_path, generated).unwrap();

    // ── themes.json bundling (Phase 2a/A3a) ───────────────────────
    // Mirror of vauchi-platform/build.rs. Materializes
    // themes/generated/themes.json into OUT_DIR/themes.json so
    // theme::bundled_themes() can include_bytes! it.
    println!("cargo::rerun-if-env-changed=VAUCHI_THEMES_DIR");

    let themes_dest = Path::new(&out_dir).join("themes.json");

    let themes_env = env::var("VAUCHI_THEMES_DIR")
        .ok()
        .map(|dir| format!("{}/generated/themes.json", dir));

    let themes_relative = [
        "../../themes/generated/themes.json",
        "../../../themes/generated/themes.json",
        "themes/generated/themes.json",
    ];

    let themes_candidates: Vec<String> = themes_env
        .into_iter()
        .chain(themes_relative.iter().map(|s| s.to_string()))
        .collect();

    let mut themes_resolved: Option<String> = None;
    for path in &themes_candidates {
        if Path::new(path).exists() {
            println!("cargo::rerun-if-changed={}", path);
            themes_resolved = Some(path.clone());
            break;
        }
    }

    let themes_bytes = match themes_resolved {
        Some(path) => {
            eprintln!("cargo:warning=Bundling themes.json from: {}", path);
            fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path, e))
        }
        None => {
            eprintln!(
                "cargo:warning=themes.json not found via VAUCHI_THEMES_DIR or sibling-repo paths — using empty `[]` fallback (runtime default_theme() will be used)"
            );
            b"[]".to_vec()
        }
    };

    fs::write(&themes_dest, themes_bytes).expect("write OUT_DIR/themes.json");

    // ── tokens.json bundling (ADR-038 Amendment 2) ───────────────
    // Materializes themes/tokens.json into OUT_DIR/tokens.json so
    // `theme::DesignTokens::default()` can include_bytes! + parse it at
    // runtime (no generated tokens_defaults.rs). NOTE: tokens.json is at
    // the themes-repo ROOT, not generated/ (unlike themes.json).
    println!("cargo::rerun-if-env-changed=VAUCHI_THEMES_DIR");

    let tokens_dest = Path::new(&out_dir).join("tokens.json");

    let tokens_env = env::var("VAUCHI_THEMES_DIR")
        .ok()
        .map(|dir| format!("{}/tokens.json", dir));

    let tokens_relative = [
        "../../themes/tokens.json",
        "../../../themes/tokens.json",
        "themes/tokens.json",
    ];

    let tokens_candidates: Vec<String> = tokens_env
        .into_iter()
        .chain(tokens_relative.iter().map(|s| s.to_string()))
        .collect();

    let mut tokens_resolved: Option<String> = None;
    for path in &tokens_candidates {
        if Path::new(path).exists() {
            println!("cargo::rerun-if-changed={}", path);
            tokens_resolved = Some(path.clone());
            break;
        }
    }

    let tokens_bytes = match tokens_resolved {
        Some(path) => {
            eprintln!("cargo:warning=Bundling tokens.json from: {}", path);
            fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path, e))
        }
        None => {
            eprintln!(
                "cargo:warning=tokens.json not found via VAUCHI_THEMES_DIR or sibling-repo paths — using frozen embedded fallback"
            );
            FROZEN_TOKENS_JSON.as_bytes().to_vec()
        }
    };

    fs::write(&tokens_dest, tokens_bytes).expect("write OUT_DIR/tokens.json");
}
