// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Build script for vauchi-app.
//!
//! Embeds two compile-time bundles into `OUT_DIR`:
//!
//! 1. **`bundled_locale.rs`** — the English, German, French, Spanish and
//!    Italian locale files from the sibling `locales/` repo (or the
//!    crate's vendored copy). They render before any runtime locale file
//!    or content cycle arrives (private#1246).
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
const FROZEN_TOKENS_JSON: &str = r##"{
  "_spdx": "SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>; SPDX-License-Identifier: AGPL-3.0-or-later",
  "version": "2.0.0",
  "spacing": { "xxs": 2, "xs": 4, "xs_sm": 6, "sm": 8, "sm_smd": 10, "sm_md": 12, "smd_md": 14, "md": 16, "md_lg": 20, "lg": 24, "xl": 32 },
  "spacing_direction": { "content_start": 16, "content_end": 16, "list_item_start": 8, "list_item_end": 8, "list_item_inline_start": 12, "list_item_inline_end": 12 },
  "typography": { "title_size": 24, "subtitle_size": 18, "body_size": 16, "caption_size": 14, "caption_sm": 12, "title_lg": 20, "display": 32, "medium_size": 20, "title_line": 30, "subtitle_line": 24, "medium_line": 28, "body_line": 24, "caption_line": 20, "text_scale_percent": 100 },
  "border_radius": { "xs": 2, "sm": 4, "md": 8, "md_lg": 12, "lg": 16, "chip": 12, "card": 20, "sheet": 28, "pill": 999 },
  "touch_target": { "minimum": 48 },
  "font_family": { "display": "Bricolage Grotesque", "body": "Hanken Grotesk", "mono": "JetBrains Mono" },
  "font_weight": { "regular": 400, "medium": 500, "semibold": 600, "bold": 700, "extrabold": 800 },
  "focus": { "ring_width": 3, "ring_offset": 2 },
  "motion": { "enter_duration_ms": 200, "exit_duration_ms": 150, "emphasis_duration_ms": 300 },
  "avatar": { "fallback_bg": "#4682B4" },
  "image": { "hero_size": 88 }
}"##;

/// Release bindings are built in tag pipelines; a dev-only feature there
/// would ship a test content-publisher key or a native log backend
/// (private#794).
fn refuse_dev_build_in_tag_pipeline() {
    println!("cargo::rerun-if-env-changed=CI_COMMIT_TAG");
    let dev_build = env::var_os("CARGO_FEATURE_DEV_BUILD").is_some();
    let tag = env::var("CI_COMMIT_TAG").unwrap_or_default();
    assert!(
        !dev_build || tag.is_empty(),
        "tag pipeline {tag} builds with a dev-only feature (dev-logging or \
         dev-local-content); release bindings must not carry one (private#794)"
    );
}

fn main() {
    refuse_dev_build_in_tag_pipeline();

    let out_dir = env::var("OUT_DIR").unwrap();
    bundle_locales(&out_dir);

    // ── themes.json bundling (Phase 2a/A3a) ───────────────────────
    // Materializes themes/generated/themes.json into OUT_DIR/themes.json so
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
                "cargo:warning=themes.json not found via VAUCHI_THEMES_DIR or sibling-repo paths — using empty `[]` fallback (runtime default_theme() will be used, and theme and fixture tests will fail). themes/generated/ is gitignored: run `python3 themes/scripts/generate.py` from the workspace root, or `just setup` (private#879)"
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

/// Resolves the `locales` revision that supplied `locale_path`.
///
/// A `REVISION` file beside the catalogue wins over git. The vendored copy
/// lives inside THIS repo, so asking git about its directory would answer
/// with core's own revision and misreport it as the locales revision —
/// `vendor-locales.sh` writes the marker so the copy carries its true
/// provenance.
///
/// Best-effort otherwise: a missing git or a tarball checkout yields
/// `None` and the build records `unknown` rather than failing. This is a
/// record, not a gate — nothing downstream may depend on it resolving.
/// The languages Core embeds, English first. A shell's files or a content
/// cycle still replace each of them and add any other (private#1246).
const EMBEDDED_LOCALES: [&str; 5] = ["en", "de", "fr", "es", "it"];

/// English when no locales checkout is found, so the crate still builds.
const FALLBACK_EN_JSON: &str = r#"{"app.name":"Vauchi","welcome.title":"Welcome to Vauchi"}"#;

/// Writes each embedded language beside `bundled_locale.rs` and the table
/// that `include_str!`s them. The files come from the first locales
/// directory holding `en.json`: `VAUCHI_LOCALES_DIR`, the sibling repo, or
/// the crate's vendored copy.
fn bundle_locales(out_dir: &str) {
    // Without the rerun declaration, cargo caches the bundled copy across
    // env changes: a build made without the var keeps serving a stale
    // vendored file even after the var is set (surfaced as phantom
    // 'Missing: <key>' smoke-test failures, 2026-07-11).
    println!("cargo::rerun-if-env-changed=VAUCHI_LOCALES_DIR");
    let candidates = env::var("VAUCHI_LOCALES_DIR").ok().into_iter().chain(
        // Sibling repo, alternative layout, the crate's vendored copy.
        ["../../locales", "../../../locales", "locales"]
            .iter()
            .map(|dir| dir.to_string()),
    );
    let found_dir = candidates
        .into_iter()
        .find(|dir| Path::new(&format!("{dir}/en.json")).exists());

    let locales_rev = found_dir.as_deref().map_or_else(
        || "none".to_string(),
        |dir| locales_revision(&format!("{dir}/en.json")).unwrap_or_else(|| "unknown".to_string()),
    );

    let mut embedded = Vec::new();
    let mut entries = String::new();
    for code in EMBEDDED_LOCALES {
        let path = found_dir.as_deref().map(|dir| format!("{dir}/{code}.json"));
        let content = path.as_deref().and_then(|path| {
            Path::new(path).exists().then(|| {
                println!("cargo::rerun-if-changed={path}");
                fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"))
            })
        });
        let content = match content {
            Some(content) => content,
            None if code == "en" => FALLBACK_EN_JSON.to_string(),
            None => continue,
        };
        let file = format!("bundled-{code}.json");
        fs::write(Path::new(out_dir).join(&file), content).unwrap();
        entries.push_str(&format!(
            "    (\"{code}\", include_str!(concat!(env!(\"OUT_DIR\"), \"/{file}\"))),\n"
        ));
        embedded.push(code);
    }

    match found_dir.as_deref() {
        Some(dir) => eprintln!(
            "cargo:warning=Bundling locales {} from: {dir} (rev {locales_rev})",
            embedded.join(",")
        ),
        None => eprintln!(
            "cargo:warning=No locales checkout found (VAUCHI_LOCALES_DIR or ../locales beside core): bundling a two-string English fallback, so i18n tests will fail. Fix: git clone git@gitlab.com:vauchi/locales.git beside core, or run `just setup` (private#879)"
        ),
    }

    let generated = format!(
        r#"/// The locale files Core embeds, by code, English first (embedded at
/// compile time from the locales repo or the crate's vendored copy).
pub const BUNDLED_LOCALE_JSON: &[(&str, &str)] = &[
{entries}];

/// Bundled English locale JSON.
pub const BUNDLED_EN_JSON: &str = BUNDLED_LOCALE_JSON[0].1;

/// Revision of the `locales` checkout this build embedded, or `unknown`
/// when the checkout is not a git working tree, or `none` when no locale
/// file was found at all.
///
/// Recorded, never enforced: a release can answer which copy it shipped
/// without freezing what developers get. Pinning would stall translations
/// behind a manual bump — see
/// `problems/2026-08-07-locale-content-consumed-from-unpinned-head/`.
pub const BUNDLED_LOCALES_REV: &str = "{locales_rev}";
"#
    );
    fs::write(Path::new(out_dir).join("bundled_locale.rs"), generated).unwrap();
}

fn locales_revision(locale_path: &str) -> Option<String> {
    let dir = Path::new(locale_path).parent()?;

    let marker = dir.join("REVISION");
    if let Ok(rev) = fs::read_to_string(&marker) {
        let rev = rev.trim().to_string();
        if !rev.is_empty() {
            println!("cargo::rerun-if-changed={}", marker.display());
            return Some(rev);
        }
    }

    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let rev = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if rev.is_empty() { None } else { Some(rev) }
}
