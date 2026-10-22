// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The languages the trusted cached manifest lists are offered before their
//! files arrive (vauchi/private#733 D3); a manifest the publisher key did
//! not sign lists nothing. The i18n store is process-global; nextest runs
//! each test in its own process.

use std::collections::HashMap;
use tempfile::TempDir;
use vauchi_app::content::{
    ContentCache, ContentConfig, ContentIndex, ContentManager, ContentManifest, FileEntry,
    LocalesEntry,
};
use vauchi_app::i18n::get_available_locales;
use vauchi_core::crypto::signing::SigningKeyPair;

fn signed_locales_manifest(signer: &SigningKeyPair, codes: &[&str]) -> ContentManifest {
    let files: HashMap<String, FileEntry> = codes
        .iter()
        .map(|code| {
            (
                code.to_string(),
                FileEntry {
                    path: format!("{code}.json"),
                    checksum: vauchi_app::content::compute_checksum(code.as_bytes()),
                    size_bytes: 2,
                },
            )
        })
        .collect();
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
            ..ContentIndex::default()
        },
        signature: None,
    };
    let canonical = serde_json::to_vec(&serde_json::to_value(&manifest).unwrap()).unwrap();
    manifest.signature = Some(hex::encode(signer.sign(&canonical).as_bytes()));
    manifest
}

fn manager_with_cached(
    manifest: &ContentManifest,
    publisher: &SigningKeyPair,
) -> (TempDir, ContentManager) {
    let temp = TempDir::new().unwrap();
    ContentCache::new(temp.path())
        .unwrap()
        .save_manifest(manifest)
        .unwrap();
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        remote_updates_enabled: false,
        publisher_public_key: publisher.public_key(),
        ..Default::default()
    };
    let manager = ContentManager::new(config, vauchi_core::clock::SystemClock::shared()).unwrap();
    (temp, manager)
}

fn offered_codes() -> Vec<String> {
    get_available_locales()
        .iter()
        .map(|l| l.code().to_string())
        .collect()
}

// @internal
#[test]
fn the_trusted_cached_manifest_lists_its_locale_codes() {
    let publisher = SigningKeyPair::generate();
    let (_temp, manager) = manager_with_cached(
        &signed_locales_manifest(&publisher, &["en", "pt"]),
        &publisher,
    );

    let mut listed = manager.listed_locale_codes();
    listed.sort();

    assert_eq!(listed, ["en", "pt"]);
}

// @internal
#[test]
fn a_manifest_another_key_signed_lists_nothing() {
    let publisher = SigningKeyPair::generate();
    let impostor = SigningKeyPair::generate();
    let (_temp, manager) = manager_with_cached(
        &signed_locales_manifest(&impostor, &["en", "pt"]),
        &publisher,
    );

    assert_eq!(manager.listed_locale_codes(), Vec::<String>::new());

    manager.load_cached_locales_into_i18n();
    assert_eq!(offered_codes(), ["en"]);
}

// @internal
#[test]
fn loading_cached_locales_offers_the_listed_languages_without_their_files() {
    let publisher = SigningKeyPair::generate();
    let (_temp, manager) = manager_with_cached(
        &signed_locales_manifest(&publisher, &["en", "pt"]),
        &publisher,
    );

    manager.load_cached_locales_into_i18n();

    assert_eq!(offered_codes(), ["en", "pt"]);
}
