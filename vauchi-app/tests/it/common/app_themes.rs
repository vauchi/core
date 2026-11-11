// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Theme catalogue helpers. They read `vauchi_app::theme`, so they stay
//! out of `vauchi-core-testkit` and go to vauchi-app's tests with the
//! theme tests that call them (vauchi/private#1230).

use vauchi_app::theme::{Theme, load_themes_from_json};

/// The themes checkout: `VAUCHI_THEMES_DIR` when the host set it (CI, and
/// cargo-mutants' relocated copy of the tree, where the manifest-relative
/// path points nowhere), else the sibling checkout.
fn themes_checkout_dir() -> std::path::PathBuf {
    match std::env::var_os("VAUCHI_THEMES_DIR") {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR")
                .expect("CARGO_MANIFEST_DIR is set by cargo test and cargo nextest"),
        )
        .join("../../themes"),
    }
}

/// Load the theme catalogue from `<dir>/generated/themes.json`, or fall back
/// to the catalogue bundled into vauchi-app at build time. The fallback is the
/// whole bundle, not one default theme, so a lookup by id behaves the same
/// with and without the checkout (private#974: the mutation baseline ran in a
/// copy of the tree without the checkout and two palette tests panicked).
pub fn themes_from_checkout_or_bundle(dir: &std::path::Path) -> Vec<Theme> {
    match std::fs::read(dir.join("generated/themes.json")) {
        Ok(data) => load_themes_from_json(&data).expect("generated/themes.json must be valid"),
        Err(_) => vauchi_app::theme::bundled_themes(),
    }
}

/// Load all themes from the checkout, else the bundled catalogue.
pub fn all_themes() -> Vec<Theme> {
    themes_from_checkout_or_bundle(&themes_checkout_dir())
}

/// Load themes from the checkout's generated/themes.json.
/// Returns None if the file does not exist (themes repo not checked out / worktree).
pub fn try_all_themes() -> Option<Vec<Theme>> {
    let data = std::fs::read(themes_checkout_dir().join("generated/themes.json")).ok()?;
    Some(load_themes_from_json(&data).expect("generated/themes.json must be valid"))
}

/// Find a theme by ID from themes.json (replaces deprecated get_theme_by_id).
pub fn theme_by_id(id: &str) -> Option<Theme> {
    all_themes().into_iter().find(|t| t.id == id)
}
