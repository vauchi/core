// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shells that drive AppEngine directly (tui, linux-gtk) get CDN content
//! from one call: cached content loads at once, Core runs the update cycle
//! within the person's setting, and applied content announces itself as a
//! Vauchi event so the shell re-renders as for any background change
//! (vauchi/private#733 D10). The string store is process-global; nextest
//! runs each test in its own process.

use std::collections::HashMap;
use std::sync::{Arc, mpsc};
use std::time::Duration;
use vauchi_app::content::{
    ContentCache, ContentEntry, ContentIndex, ContentManifest, ContentSource, ContentSourceHandle,
    ContentType, FileEntry, LocalesEntry, compute_checksum,
};
use vauchi_app::i18n::{Locale, try_get_string};
use vauchi_app::ui::{AppEngine, ContentRunOptions};
use vauchi_core::Command;
use vauchi_core::api::{Vauchi, VauchiEvent};
use vauchi_core::crypto::signing::SigningKeyPair;

const GERMAN: &[u8] = br#"{"_meta":{"locale":"de"},"runner.probe":"aus dem Relay"}"#;

const THEMES: &[u8] = br##"[{
    "id": "cdn-only","name": "From the CDN","version": "1.0.0","mode": "dark",
    "colors": {
        "bg-primary": "#010203","bg-secondary": "#111111","bg-tertiary": "#222222",
        "text-primary": "#ffffff","text-secondary": "#cccccc",
        "accent": "#0000ff","accent-dark": "#000099",
        "success": "#00ff00","error": "#ff0000","warning": "#ffff00","border": "#333333"
    }
}]"##;

struct Files(HashMap<String, Vec<u8>>);

impl ContentSource for Files {
    fn fetch(&self, path: &str) -> Result<Vec<u8>, String> {
        self.0.get(path).cloned().ok_or_else(|| "missing".into())
    }
}

fn signed_manifest(signer: &SigningKeyPair, with_themes: bool) -> Vec<u8> {
    let files = HashMap::from([(
        "de".to_string(),
        FileEntry {
            path: "de.json".to_string(),
            checksum: compute_checksum(GERMAN),
            size_bytes: GERMAN.len() as u64,
            name: None,
            english_name: None,
            is_rtl: None,
        },
    )]);
    let mut manifest = ContentManifest {
        schema_version: 1,
        generated_at: "2026-10-22T00:00:00Z".to_string(),
        base_url: "https://cdn.vauchi.app/v1/".to_string(),
        content: ContentIndex {
            locales: Some(LocalesEntry {
                version: "1.0.0".to_string(),
                path: "locales/".to_string(),
                files,
                min_app_version: "0.1.0".to_string(),
            }),
            themes: with_themes.then(|| ContentEntry {
                version: "1.0.0".to_string(),
                path: "themes.json".to_string(),
                checksum: compute_checksum(THEMES),
                size_bytes: THEMES.len() as u64,
                min_app_version: "0.1.0".to_string(),
                max_app_version: None,
            }),
            ..ContentIndex::default()
        },
        signature: None,
    };
    let canonical = serde_json::to_vec(&serde_json::to_value(&manifest).unwrap()).unwrap();
    manifest.signature = Some(hex::encode(signer.sign(&canonical).as_bytes()));
    serde_json::to_vec(&manifest).unwrap()
}

fn engine_picking_german() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.set_render_context(vauchi_app::ui::RenderContext {
        locale: Some("de".into()),
        ..Default::default()
    });
    engine
}

// @internal
#[test]
fn content_cached_by_an_earlier_run_loads_when_updates_start() {
    let dir = tempfile::tempdir().unwrap();
    ContentCache::new(dir.path())
        .unwrap()
        .save_content(
            ContentType::Locales,
            "de.json",
            GERMAN,
            &compute_checksum(GERMAN),
        )
        .unwrap();
    let mut engine = engine_picking_german();

    engine.start_content_updates(dir.path(), ContentRunOptions::default());

    assert_eq!(
        try_get_string(Locale::GERMAN, "runner.probe").as_deref(),
        Some("aus dem Relay")
    );
}

/// Starts the runner over a relay serving German strings, and themes when
/// `with_themes`, and waits until the update announces itself.
fn engine_after_an_update(dir: &std::path::Path, with_themes: bool) -> AppEngine {
    let publisher = SigningKeyPair::generate();
    let mut files = HashMap::from([
        (
            "manifest.json".to_string(),
            signed_manifest(&publisher, with_themes),
        ),
        ("locales/de.json".to_string(), GERMAN.to_vec()),
    ]);
    if with_themes {
        files.insert("themes.json".to_string(), THEMES.to_vec());
    }
    let mut engine = engine_picking_german();
    let (updated, announced) = mpsc::channel();
    engine
        .vauchi()
        .add_event_handler(Arc::new(move |event: VauchiEvent| {
            if matches!(event, VauchiEvent::ContentUpdated) {
                let _ = updated.send(());
            }
        }));

    engine.start_content_updates(
        dir,
        ContentRunOptions {
            first_run_after: Duration::ZERO,
            source: Some(ContentSourceHandle(Arc::new(Files(files)))),
            publisher_public_key: Some(publisher.public_key()),
            ..ContentRunOptions::default()
        },
    );

    assert_eq!(announced.recv_timeout(Duration::from_secs(10)), Ok(()));
    engine
}

fn reload_theme_count(commands: &[Command]) -> usize {
    commands
        .iter()
        .filter(|command| matches!(command, Command::ReloadTheme))
        .count()
}

// @internal
#[test]
fn applied_content_loads_and_announces_itself() {
    let dir = tempfile::tempdir().unwrap();

    let _engine = engine_after_an_update(dir.path(), false);

    assert_eq!(
        try_get_string(Locale::GERMAN, "runner.probe").as_deref(),
        Some("aus dem Relay")
    );
}

/// Shells apply the theme natively, so new themes reach the screen only
/// when Core asks them to reload it (vauchi/private#733).
// @internal
#[test]
fn applied_themes_ask_the_shell_once_to_reload_its_theme() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine_after_an_update(dir.path(), true);

    let first = engine.initial_commands().unwrap();
    let second = engine.initial_commands().unwrap();

    assert_eq!(reload_theme_count(&first), 1, "{first:?}");
    assert_eq!(reload_theme_count(&second), 0, "{second:?}");
    let ids: Vec<String> = vauchi_app::theme::active_themes()
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, vec!["cdn-only".to_string()]);
}

// @internal
#[test]
fn applied_strings_alone_leave_the_theme_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = engine_after_an_update(dir.path(), false);

    let commands = engine.initial_commands().unwrap();

    assert_eq!(reload_theme_count(&commands), 0, "{commands:?}");
}

// @internal
#[test]
fn a_content_update_affects_every_main_screen() {
    let screens = vauchi_app::ui::affected_screens(&VauchiEvent::ContentUpdated);

    for screen in ["my_info", "contacts", "settings"] {
        assert!(screens.contains(&screen), "{screen}: {screens:?}");
    }
}
