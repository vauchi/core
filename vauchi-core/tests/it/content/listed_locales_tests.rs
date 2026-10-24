// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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
use vauchi_app::i18n::{Locale, get_available_locales, get_locale_info};
use vauchi_core::crypto::signing::SigningKeyPair;

fn unnamed(code: &str) -> (&str, Option<(&str, &str, bool)>) {
    (code, None)
}

fn signed_locales_manifest(signer: &SigningKeyPair, codes: &[&str]) -> ContentManifest {
    let entries: Vec<_> = codes.iter().map(|code| unnamed(code)).collect();
    signed_named_locales_manifest(signer, &entries)
}

fn signed_named_locales_manifest(
    signer: &SigningKeyPair,
    entries: &[(&str, Option<(&str, &str, bool)>)],
) -> ContentManifest {
    let files: HashMap<String, FileEntry> = entries
        .iter()
        .map(|(code, names)| {
            (
                code.to_string(),
                FileEntry {
                    path: format!("{code}.json"),
                    checksum: vauchi_app::content::compute_checksum(code.as_bytes()),
                    size_bytes: 2,
                    name: names.map(|(name, _, _)| name.to_string()),
                    english_name: names.map(|(_, english, _)| english.to_string()),
                    is_rtl: names.map(|(_, _, rtl)| rtl),
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

/// Settings names a listed language before its file arrives, from the
/// signed manifest (vauchi/private#733 D7).
// @internal
#[test]
fn a_listed_language_is_named_by_the_manifest_before_its_file_arrives() {
    let publisher = SigningKeyPair::generate();
    let manifest = signed_named_locales_manifest(
        &publisher,
        &[("pt", Some(("Português", "Portuguese", false)))],
    );
    let (_temp, manager) = manager_with_cached(&manifest, &publisher);

    manager.load_cached_locales_into_i18n();

    let info = get_locale_info(Locale::from_code("pt").unwrap());
    assert_eq!(
        (info.name.as_str(), info.english_name.as_str()),
        ("Português", "Portuguese")
    );
}

// @internal
#[test]
fn an_unusable_listed_name_names_the_language_by_its_code() {
    let publisher = SigningKeyPair::generate();
    let long = "n".repeat(65);
    let manifest = signed_named_locales_manifest(
        &publisher,
        &[
            ("sv", Some((long.as_str(), "Swedish", false))),
            ("pl", Some(("Pol\u{7}ski", "Polish", false))),
        ],
    );
    let (_temp, manager) = manager_with_cached(&manifest, &publisher);

    manager.load_cached_locales_into_i18n();

    assert_eq!(get_locale_info(Locale::from_code("sv").unwrap()).name, "sv");
    assert_eq!(get_locale_info(Locale::from_code("pl").unwrap()).name, "pl");
}

// @internal
#[test]
fn a_listed_right_to_left_language_is_not_offered() {
    let publisher = SigningKeyPair::generate();
    let manifest =
        signed_named_locales_manifest(&publisher, &[("fa", Some(("فارسی", "Persian", true)))]);
    let (_temp, manager) = manager_with_cached(&manifest, &publisher);

    manager.load_cached_locales_into_i18n();

    assert_eq!(offered_codes(), ["en"]);
}

// @internal
#[test]
fn a_name_changed_after_signing_fails_the_signature() {
    let publisher = SigningKeyPair::generate();
    let mut manifest = signed_named_locales_manifest(
        &publisher,
        &[("pt", Some(("Português", "Portuguese", false)))],
    );
    let files = &mut manifest.content.locales.as_mut().unwrap().files;
    files.get_mut("pt").unwrap().name = Some("Fake".to_string());
    let (_temp, manager) = manager_with_cached(&manifest, &publisher);

    assert_eq!(manager.listed_locale_codes(), Vec::<String>::new());
}
