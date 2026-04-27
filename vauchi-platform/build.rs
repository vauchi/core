// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Build script for vauchi-platform.
//!
//! Materializes `themes/generated/themes.json` into `OUT_DIR/themes.json`
//! so `lib.rs` can `include_bytes!(concat!(env!("OUT_DIR"), "/themes.json"))`
//! without a path relative to a *sibling* directory of the cargo workspace.
//!
//! Without this script, the const used a `../../../themes/generated/themes.json`
//! path that only resolved when the build ran inside the in-repo tree.
//! cargo-mutants (without `--in-place`) copies the workspace to a temp dir
//! and the sibling `themes/` is not preserved, so the build failed for
//! every mutant in the entire workspace — even ones in unrelated files.
//! That forced `--in-place`, which in turn forbade `--jobs N` parallelism
//! (cargo-mutants 26 disallows the combo).
//!
//! The script searches in this order:
//!   1. `$VAUCHI_THEMES_DIR/generated/themes.json` (CI sets the absolute
//!      path of the cloned `themes/` repo)
//!   2. `../../themes/generated/themes.json` (in-repo workspace layout)
//!   3. `../../../themes/generated/themes.json` (alternate layout)
//!   4. `themes/generated/themes.json` (local copy, e.g. for crates.io
//!      publish)
//!
//! If none are found, a minimal `[]` fallback is written. At runtime,
//! `vauchi_app::theme::load_themes_from_json` will return Err and the
//! caller falls back to `default_theme()`. This matches the existing
//! locale-bundle fallback in `vauchi-core/build.rs`.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo::rerun-if-env-changed=VAUCHI_THEMES_DIR");

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR set by cargo");
    let dest_path = Path::new(&out_dir).join("themes.json");

    let env_path = env::var("VAUCHI_THEMES_DIR")
        .ok()
        .map(|dir| format!("{}/generated/themes.json", dir));

    let relative_paths = [
        "../../themes/generated/themes.json",
        "../../../themes/generated/themes.json",
        "themes/generated/themes.json",
    ];

    let candidates: Vec<String> = env_path
        .into_iter()
        .chain(relative_paths.iter().map(|s| s.to_string()))
        .collect();

    let mut resolved: Option<String> = None;
    for path in &candidates {
        if Path::new(path).exists() {
            println!("cargo::rerun-if-changed={}", path);
            resolved = Some(path.clone());
            break;
        }
    }

    let bytes = match resolved {
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

    fs::write(&dest_path, bytes).expect("write OUT_DIR/themes.json");
}
