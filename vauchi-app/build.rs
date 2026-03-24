// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Build script for vauchi-core
//!
//! Embeds the English locale from the sibling locales repo at compile time.
//! This provides a complete fallback when locale files aren't loaded at runtime.

use std::env;
use std::fs;
use std::path::Path;

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
}
