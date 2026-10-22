// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use tempfile::TempDir;
use vauchi_core::crypto::signing::SigningKeyPair;

fn signed_networks_manifest(keypair: &SigningKeyPair, version: &str) -> ContentManifest {
    let mut manifest = ContentManifest {
        schema_version: 1,
        generated_at: "2026-07-13T00:00:00Z".to_string(),
        base_url: "https://cdn.vauchi.app/v1/".to_string(),
        content: super::super::types::ContentIndex {
            networks: Some(super::super::types::ContentEntry {
                version: version.to_string(),
                path: "networks.json".to_string(),
                checksum: super::super::integrity::compute_checksum(b"expected content"),
                size_bytes: 16,
                min_app_version: "0.1.0".to_string(),
                max_app_version: None,
            }),
            ..super::super::types::ContentIndex::default()
        },
        signature: None,
    };

    let canonical = serde_json::to_vec(&serde_json::to_value(&manifest).unwrap()).unwrap();
    manifest.signature = Some(hex::encode(keypair.sign(&canonical).as_bytes()));
    manifest
}

fn serve_http(responses: Vec<Vec<u8>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();

    thread::spawn(move || {
        for body in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
    });

    format!("http://{address}")
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(future)
}

// @internal
#[test]
fn test_bundled_english_locale() {
    let locale = bundled_locale("en");
    assert!(locale.is_some(), "expected Some value");
    let strings = locale.unwrap();
    assert!(strings.contains_key("app.name"));
}

// @internal
#[test]
fn test_bundled_unknown_locale() {
    let locale = bundled_locale("zz");
    assert!(locale.is_none());
}

// Scenario: Reject a manifest without a publisher signature
// @scenario: remote_content :: Reject a manifest without a publisher signature
#[test]
fn test_manager_rejects_unsigned_manifest() {
    let temp = TempDir::new().unwrap();
    let unsigned = ContentManifest {
        schema_version: 1,
        generated_at: "2026-07-13T00:00:00Z".to_string(),
        base_url: "https://cdn.vauchi.app/v1/".to_string(),
        content: super::super::types::ContentIndex::default(),
        signature: None,
    };
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        content_url: serve_http(vec![serde_json::to_vec(&unsigned).unwrap()]),
        ..Default::default()
    };
    let manager = ContentManager::new(config, vauchi_core::clock::SystemClock::shared()).unwrap();

    let status = block_on(manager.check_for_updates());

    assert!(
        matches!(status, UpdateStatus::CheckFailed(message) if message.contains("signature")),
        "unsigned manifests must fail closed"
    );
}

// Scenario: Reject a signed content downgrade
// @scenario: remote_content :: Reject a signed content downgrade
#[test]
fn test_manager_rejects_signed_content_downgrade() {
    let temp = TempDir::new().unwrap();
    let keypair = SigningKeyPair::generate();
    let cached = signed_networks_manifest(&keypair, "1.1.0");
    ContentCache::new(temp.path())
        .unwrap()
        .save_manifest(&cached)
        .unwrap();
    let remote = signed_networks_manifest(&keypair, "1.0.0");
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        content_url: serve_http(vec![serde_json::to_vec(&remote).unwrap()]),
        publisher_public_key: keypair.public_key(),
        ..Default::default()
    };
    let manager = ContentManager::new(config, vauchi_core::clock::SystemClock::shared()).unwrap();

    let status = block_on(manager.check_for_updates());

    assert!(matches!(status, UpdateStatus::UpToDate));
}

// @internal
#[test]
fn test_manager_ignores_untrusted_cached_rollback_anchor() {
    let temp = TempDir::new().unwrap();
    let trusted_signer = SigningKeyPair::generate();
    let untrusted_signer = SigningKeyPair::generate();
    let poisoned_cache = signed_networks_manifest(&untrusted_signer, "999.0.0");
    ContentCache::new(temp.path())
        .unwrap()
        .save_manifest(&poisoned_cache)
        .unwrap();
    let remote = signed_networks_manifest(&trusted_signer, "1.0.0");
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        content_url: serve_http(vec![serde_json::to_vec(&remote).unwrap()]),
        publisher_public_key: trusted_signer.public_key(),
        ..Default::default()
    };
    let manager = ContentManager::new(config, vauchi_core::clock::SystemClock::shared()).unwrap();

    let status = block_on(manager.check_for_updates());

    assert!(matches!(
        status,
        UpdateStatus::UpdatesAvailable(types) if types == vec![ContentType::Networks]
    ));
}

// Scenario: Preserve the cached version when a content download fails
// @scenario: remote_content :: Preserve the cached version when a content download fails
#[test]
fn test_manager_does_not_advance_manifest_after_failed_download() {
    let temp = TempDir::new().unwrap();
    let keypair = SigningKeyPair::generate();
    let remote = signed_networks_manifest(&keypair, "1.1.0");
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        content_url: serve_http(vec![
            serde_json::to_vec(&remote).unwrap(),
            b"tampered content".to_vec(),
        ]),
        publisher_public_key: keypair.public_key(),
        ..Default::default()
    };
    let manager = ContentManager::new(config, vauchi_core::clock::SystemClock::shared()).unwrap();

    let result = block_on(manager.apply_updates()).unwrap();

    assert!(matches!(
        result,
        ApplyResult::Applied { ref failed, .. } if failed.len() == 1
    ));
    assert!(
        manager.cache().get_manifest().is_none(),
        "a failed content download must remain retryable"
    );
}

// A download that matches its checksum but does not load is not cached:
// cached, it would fail again at every start while the manifest says the
// content is current, so nothing would ever download it again (#747).
// @internal
#[test]
fn test_manager_does_not_cache_themes_that_do_not_load() {
    let temp = TempDir::new().unwrap();
    let keypair = SigningKeyPair::generate();
    let catalog = b"not a theme catalog".to_vec();
    let mut remote = ContentManifest {
        schema_version: 1,
        generated_at: "2026-10-21T00:00:00Z".to_string(),
        base_url: "https://cdn.vauchi.app/v1/".to_string(),
        content: super::super::types::ContentIndex {
            themes: Some(super::super::types::ContentEntry {
                version: "1.0.0".to_string(),
                path: "themes.json".to_string(),
                checksum: super::super::integrity::compute_checksum(&catalog),
                size_bytes: catalog.len() as u64,
                min_app_version: "0.1.0".to_string(),
                max_app_version: None,
            }),
            ..super::super::types::ContentIndex::default()
        },
        signature: None,
    };
    let canonical = serde_json::to_vec(&serde_json::to_value(&remote).unwrap()).unwrap();
    remote.signature = Some(hex::encode(keypair.sign(&canonical).as_bytes()));
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        content_url: serve_http(vec![serde_json::to_vec(&remote).unwrap(), catalog]),
        publisher_public_key: keypair.public_key(),
        ..Default::default()
    };
    let manager = ContentManager::new(config, vauchi_core::clock::SystemClock::shared()).unwrap();

    let result = block_on(manager.apply_updates()).unwrap();

    assert!(matches!(
        result,
        ApplyResult::Applied { ref applied, ref failed }
            if applied.is_empty() && failed.len() == 1 && failed[0].0 == ContentType::Themes
    ));
    assert!(
        manager
            .cache()
            .get_content(ContentType::Themes, "themes.json")
            .is_none(),
        "a catalog that does not load is not cached"
    );
    assert!(
        manager.cache().get_manifest().is_none(),
        "the themes update stays retryable"
    );
}

// @internal
#[test]
fn test_content_versions_require_exact_numeric_triplets() {
    assert_eq!(parse_exact_version("1.2.3"), Some([1, 2, 3]));
    assert_eq!(parse_exact_version("1.2"), None);
    assert_eq!(parse_exact_version("1.2.3.4"), None);
    assert_eq!(parse_exact_version("1.two.3"), None);
    assert_eq!(parse_exact_version("1.2.3-beta"), None);
}
