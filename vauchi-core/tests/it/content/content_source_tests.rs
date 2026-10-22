// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The binding layers fetch content through the relay over OHTTP by giving
//! the manager a content source (vauchi/private#733 D8); the manager still
//! checks the signed manifest, each file's checksum and its size. Here the
//! direct CDN URL is a closed port, so a request that went direct fails.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use vauchi_app::content::{
    ApplyResult, ContentCache, ContentConfig, ContentIndex, ContentManager, ContentManifest,
    ContentSource, ContentSourceHandle, ContentType, FileEntry, LocalesEntry, UpdateStatus,
    compute_checksum,
};
use vauchi_core::crypto::signing::SigningKeyPair;

const GERMAN: &[u8] = br#"{"_meta":{"locale":"de"},"source.probe":"aus dem Relay"}"#;

struct StubSource {
    files: HashMap<String, Vec<u8>>,
    asked: Mutex<Vec<String>>,
}

impl ContentSource for StubSource {
    fn fetch(&self, path: &str) -> Result<Vec<u8>, String> {
        self.asked.lock().unwrap().push(path.to_string());
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| "missing".to_string())
    }
}

fn signed_manifest(signer: &SigningKeyPair, checksum_of: &[u8]) -> Vec<u8> {
    let files = HashMap::from([(
        "de".to_string(),
        FileEntry {
            path: "de.json".to_string(),
            checksum: compute_checksum(checksum_of),
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
            ..ContentIndex::default()
        },
        signature: None,
    };
    let canonical = serde_json::to_vec(&serde_json::to_value(&manifest).unwrap()).unwrap();
    manifest.signature = Some(hex::encode(signer.sign(&canonical).as_bytes()));
    serde_json::to_vec(&manifest).unwrap()
}

fn manager_over(
    temp: &TempDir,
    publisher: &SigningKeyPair,
    files: Vec<(&str, Vec<u8>)>,
    max_content_size: u64,
) -> (ContentManager, Arc<StubSource>) {
    let source = Arc::new(StubSource {
        files: files.into_iter().map(|(p, b)| (p.to_string(), b)).collect(),
        asked: Mutex::new(Vec::new()),
    });
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        content_url: "http://127.0.0.1:9".to_string(),
        publisher_public_key: publisher.public_key(),
        max_content_size,
        source: Some(ContentSourceHandle(source.clone())),
        ..Default::default()
    };
    (
        ContentManager::new(config, vauchi_core::clock::SystemClock::shared()).unwrap(),
        source,
    )
}

// @internal
#[test]
fn a_cycle_fetches_everything_through_the_source() {
    let temp = TempDir::new().unwrap();
    let publisher = SigningKeyPair::generate();
    let (manager, source) = manager_over(
        &temp,
        &publisher,
        vec![
            ("manifest.json", signed_manifest(&publisher, GERMAN)),
            ("locales/de.json", GERMAN.to_vec()),
        ],
        1024 * 1024,
    );

    let (status, apply) = manager.run_update_cycle_blocking();

    assert!(
        matches!(status, UpdateStatus::UpdatesAvailable(_)),
        "{status:?}"
    );
    assert!(
        matches!(&apply, Some(Ok(ApplyResult::Applied { applied, failed }))
        if applied == &vec![ContentType::Locales] && failed.is_empty()),
        "{apply:?}"
    );
    assert_eq!(
        source
            .asked
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.as_str() == "locales/de.json")
            .count(),
        1
    );
}

// @internal
#[test]
fn a_file_from_the_source_with_the_wrong_checksum_is_not_cached() {
    let temp = TempDir::new().unwrap();
    let publisher = SigningKeyPair::generate();
    let (manager, _) = manager_over(
        &temp,
        &publisher,
        vec![
            (
                "manifest.json",
                signed_manifest(&publisher, b"something else"),
            ),
            ("locales/de.json", GERMAN.to_vec()),
        ],
        1024 * 1024,
    );

    let (_, apply) = manager.run_update_cycle_blocking();

    assert!(
        matches!(&apply, Some(Ok(ApplyResult::Applied { applied, .. })) if applied.is_empty()),
        "{apply:?}"
    );
    assert!(
        ContentCache::new(temp.path())
            .unwrap()
            .content_files(ContentType::Locales)
            .is_empty()
    );
}

// @internal
#[test]
fn an_oversized_file_from_the_source_is_refused() {
    let temp = TempDir::new().unwrap();
    let publisher = SigningKeyPair::generate();
    let (manager, _) = manager_over(
        &temp,
        &publisher,
        vec![
            ("manifest.json", signed_manifest(&publisher, GERMAN)),
            ("locales/de.json", GERMAN.to_vec()),
        ],
        10,
    );

    let (status, _) = manager.run_update_cycle_blocking();

    assert!(matches!(status, UpdateStatus::CheckFailed(_)), "{status:?}");
}
