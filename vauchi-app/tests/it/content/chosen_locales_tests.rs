// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Only the languages a person uses are fetched and kept: English, the
//! picked language and the system language (vauchi/private#733 D7). A
//! wanted language that is listed but not cached is fetched at the next
//! cycle, even inside the check interval. The string store is
//! process-global; nextest runs each test in its own process.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use vauchi_app::content::{
    ContentCache, ContentConfig, ContentIndex, ContentManager, ContentManifest, ContentType,
    FileEntry, LocalesEntry, UpdateStatus, compute_checksum,
};
use vauchi_core::crypto::signing::SigningKeyPair;

const LISTED: [&str; 5] = ["en", "de", "fr", "it", "es"];

fn locale_file(code: &str) -> Vec<u8> {
    format!(r#"{{"_meta":{{"locale":"{code}","name":"{code}-name","english_name":"{code}-english","is_rtl":false}},"chosen.probe":"{code}"}}"#).into_bytes()
}

fn signed_manifest(signer: &SigningKeyPair, version: &str) -> Vec<u8> {
    let files: HashMap<String, FileEntry> = LISTED
        .iter()
        .map(|code| {
            let body = locale_file(code);
            (
                code.to_string(),
                FileEntry {
                    path: format!("{code}.json"),
                    checksum: compute_checksum(&body),
                    size_bytes: body.len() as u64,
                    name: None,
                    english_name: None,
                    is_rtl: None,
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
                version: version.to_string(),
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

/// Serves the manifest and every listed file by path, and records each
/// requested path.
fn cdn(manifest: Vec<u8>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let requested = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&requested);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut request = [0_u8; 2048];
            let read = stream.read(&mut request).unwrap();
            let line = String::from_utf8_lossy(&request[..read]);
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            let body = if path.ends_with("manifest.json") {
                manifest.clone()
            } else {
                let code = path.rsplit('/').next().unwrap().trim_end_matches(".json");
                locale_file(code)
            };
            log.lock().unwrap().push(path);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
    });
    (format!("http://{address}"), requested)
}

fn manager(
    temp: &TempDir,
    url: &str,
    publisher: &SigningKeyPair,
    wanted: &[&str],
) -> ContentManager {
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        content_url: url.to_string(),
        publisher_public_key: publisher.public_key(),
        wanted_locales: Some(wanted.iter().map(|c| c.to_string()).collect()),
        ..Default::default()
    };
    ContentManager::new(config, vauchi_core::clock::SystemClock::shared()).unwrap()
}

fn fetched_locales(requested: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    let mut codes: Vec<String> = requested
        .lock()
        .unwrap()
        .iter()
        .filter(|p| !p.ends_with("manifest.json"))
        .map(|p| {
            p.rsplit('/')
                .next()
                .unwrap()
                .trim_end_matches(".json")
                .to_string()
        })
        .collect();
    codes.sort();
    codes
}

fn cached_locales(temp: &TempDir) -> Vec<String> {
    let mut names = ContentCache::new(temp.path())
        .unwrap()
        .content_files(ContentType::Locales);
    names.sort();
    names
}

// @internal
#[test]
fn a_cycle_fetches_only_the_wanted_languages() {
    let temp = TempDir::new().unwrap();
    let publisher = SigningKeyPair::generate();
    let (url, requested) = cdn(signed_manifest(&publisher, "1.0.0"));

    let (_, apply) =
        manager(&temp, &url, &publisher, &["en", "de", "fr"]).run_update_cycle_blocking();

    assert!(matches!(apply, Some(Ok(_))), "{apply:?}");
    assert_eq!(fetched_locales(&requested), ["de", "en", "fr"]);
    assert_eq!(cached_locales(&temp), ["de.json", "en.json", "fr.json"]);
}

// @internal
#[test]
fn a_wanted_language_that_is_not_listed_is_never_requested() {
    let temp = TempDir::new().unwrap();
    let publisher = SigningKeyPair::generate();
    let (url, requested) = cdn(signed_manifest(&publisher, "1.0.0"));

    let _ = manager(&temp, &url, &publisher, &["en", "pt"]).run_update_cycle_blocking();

    assert_eq!(fetched_locales(&requested), ["en"]);
}

// @internal
#[test]
fn languages_no_longer_wanted_are_deleted_from_the_cache() {
    let temp = TempDir::new().unwrap();
    let publisher = SigningKeyPair::generate();
    let (url, _) = cdn(signed_manifest(&publisher, "1.0.0"));
    let _ = manager(&temp, &url, &publisher, &["en", "it", "es"]).run_update_cycle_blocking();
    let (next_url, _) = cdn(signed_manifest(&publisher, "1.1.0"));

    let _ = manager(&temp, &next_url, &publisher, &["en", "de"]).run_update_cycle_blocking();

    assert_eq!(cached_locales(&temp), ["de.json", "en.json"]);
}

// @internal
#[test]
fn a_newly_wanted_listed_language_is_fetched_inside_the_check_interval() {
    let temp = TempDir::new().unwrap();
    let publisher = SigningKeyPair::generate();
    let (url, requested) = cdn(signed_manifest(&publisher, "1.0.0"));
    let _ = manager(&temp, &url, &publisher, &["en"]).run_update_cycle_blocking();
    requested.lock().unwrap().clear();

    let (status, _) = manager(&temp, &url, &publisher, &["en", "it"]).run_update_cycle_blocking();

    assert!(
        matches!(&status, UpdateStatus::UpdatesAvailable(types) if types == &vec![ContentType::Locales]),
        "{status:?}"
    );
    assert_eq!(fetched_locales(&requested), ["it"]);
    assert_eq!(cached_locales(&temp), ["en.json", "it.json"]);
}

// @internal
#[test]
fn with_everything_wanted_cached_a_cycle_inside_the_interval_asks_nothing() {
    let temp = TempDir::new().unwrap();
    let publisher = SigningKeyPair::generate();
    let (url, requested) = cdn(signed_manifest(&publisher, "1.0.0"));
    let _ = manager(&temp, &url, &publisher, &["en", "de"]).run_update_cycle_blocking();
    requested.lock().unwrap().clear();

    let (status, apply) =
        manager(&temp, &url, &publisher, &["en", "de"]).run_update_cycle_blocking();

    assert!(matches!(status, UpdateStatus::UpToDate), "{status:?}");
    assert!(apply.is_none());
    assert_eq!(requested.lock().unwrap().len(), 0);
}
