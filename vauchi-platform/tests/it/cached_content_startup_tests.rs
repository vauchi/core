// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Content downloaded from the CDN in an earlier run is in effect after
//! a restart (vauchi/private#736). The update cycle re-downloads nothing
//! when the cached manifest is current, so the engine must load the cache
//! when it starts. String and theme stores are process-global; nextest
//! runs each test in its own process.

use vauchi_app::i18n::{Locale, try_get_string};
use vauchi_platform::PlatformAppEngineTestHelpers;

const CACHED_THEMES: &str = r##"[{
    "id": "cdn-only","name": "From the CDN","version": "1.0.0","mode": "dark",
    "colors": {
        "bg-primary": "#010203","bg-secondary": "#111111","bg-tertiary": "#222222",
        "text-primary": "#ffffff","text-secondary": "#cccccc",
        "accent": "#0000ff","accent-dark": "#000099",
        "success": "#00ff00","error": "#ff0000","warning": "#ffff00","border": "#333333"
    }
}]"##;

fn start_engine_over(cached: &[(&str, &str)]) -> tempfile::TempDir {
    use vauchi_app::content::{ContentCache, ContentType, compute_checksum};
    let dir = tempfile::tempdir().expect("temp dir");
    let cache = ContentCache::new(dir.path()).unwrap();
    for (path, body) in cached {
        let (dir_name, filename) = path.split_once('/').unwrap();
        let content_type = [
            ContentType::Networks,
            ContentType::Locales,
            ContentType::Help,
            ContentType::Themes,
        ]
        .into_iter()
        .find(|t| t.dir_name() == dir_name)
        .unwrap();
        cache
            .save_content(
                content_type,
                filename,
                body.as_bytes(),
                &compute_checksum(body.as_bytes()),
            )
            .unwrap();
    }
    let key = vauchi_core::crypto::SymmetricKey::generate();
    vauchi_platform::PlatformAppEngine::open_under_shell_key(
        dir.path().to_string_lossy().to_string(),
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("create engine");
    dir
}

// @internal
#[test]
fn a_locale_cached_by_an_earlier_run_is_loaded_when_the_engine_starts() {
    let _dir = start_engine_over(&[(
        "locales/de.json",
        r#"{"_meta":{"locale":"de"},"cdn.probe":"vom CDN"}"#,
    )]);

    assert_eq!(
        try_get_string(Locale::GERMAN, "cdn.probe").as_deref(),
        Some("vom CDN")
    );
}

// @internal
#[test]
fn a_theme_cached_by_an_earlier_run_is_loaded_when_the_engine_starts() {
    let _dir = start_engine_over(&[("themes/themes.json", CACHED_THEMES)]);

    let ids: Vec<String> = vauchi_app::theme::active_themes()
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert!(
        ids.contains(&"cdn-only".to_string()),
        "active themes: {ids:?}"
    );
}

/// Shells apply a theme natively by asking for it, so a CDN theme picked
/// in Settings must be one they can find (vauchi/private#733).
// @internal
#[test]
fn shells_find_a_theme_cached_by_an_earlier_run() {
    let _dir = start_engine_over(&[("themes/themes.json", CACHED_THEMES)]);

    let listed: Vec<String> = vauchi_platform::get_available_themes()
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(listed, vec!["cdn-only".to_string()]);
    assert_eq!(
        vauchi_platform::get_theme("cdn-only".into()).map(|t| t.name),
        Some("From the CDN".to_string())
    );
    assert_eq!(
        vauchi_platform::get_theme("not-a-theme".into()).map(|t| t.id),
        None
    );
}

// @internal
#[test]
fn the_shells_bundled_locales_loaded_after_start_keep_the_cached_overlay() {
    let _dir = start_engine_over(&[(
        "locales/de.json",
        r#"{"_meta":{"locale":"de"},"cdn.probe":"vom CDN"}"#,
    )]);
    let bundled = tempfile::tempdir().expect("bundled dir");
    std::fs::write(
        bundled.path().join("de.json"),
        r#"{"_meta":{"locale":"de"},"bundled.probe":"gebündelt"}"#,
    )
    .unwrap();

    vauchi_platform::init_locales(bundled.path().to_string_lossy().to_string()).unwrap();

    assert_eq!(
        try_get_string(Locale::GERMAN, "cdn.probe").as_deref(),
        Some("vom CDN")
    );
    assert_eq!(
        try_get_string(Locale::GERMAN, "bundled.probe").as_deref(),
        Some("gebündelt")
    );
}

/// A network renamed on the CDN in an earlier run keeps its new name
/// after a restart (vauchi/private#734 T3.1).
// @internal
#[test]
fn networks_cached_by_an_earlier_run_are_loaded_when_the_engine_starts() {
    let _dir = start_engine_over(&[(
        "networks/networks.json",
        r#"[{"id": "twitter", "name": "X", "url": "https://x.com/{username}"}]"#,
    )]);

    assert_eq!(
        vauchi_app::social::active_registry()
            .get("twitter")
            .map(|n| n.display_name().to_string())
            .as_deref(),
        Some("X")
    );
}
