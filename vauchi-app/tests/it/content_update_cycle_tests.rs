// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! One content update cycle, as the scheduler and the binding layers run
//! it, against a content source that records every path it is asked for.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use vauchi_app::content::{
    ApplyResult, ContentCache, ContentConfig, ContentEntry, ContentIndex, ContentManager,
    ContentManifest, ContentSource, ContentSourceHandle, ContentType, LocalesEntry, UpdateStatus,
    compute_checksum,
};
use vauchi_core::clock::Clock;
use vauchi_core::crypto::signing::SigningKeyPair;

const NETWORKS: &[u8] = br#"{"networks":[]}"#;

struct PinnedClock(SystemTime);

impl Clock for PinnedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}

#[derive(Default)]
struct RecordingSource {
    files: HashMap<String, Vec<u8>>,
    requested: Mutex<Vec<String>>,
}

impl ContentSource for RecordingSource {
    fn fetch(&self, path: &str) -> Result<Vec<u8>, String> {
        self.requested.lock().unwrap().push(path.to_string());
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| "missing".into())
    }
}

impl RecordingSource {
    fn serving(manifest: &ContentManifest, networks: &[u8]) -> Arc<Self> {
        Arc::new(Self {
            files: HashMap::from([
                (
                    "manifest.json".to_string(),
                    serde_json::to_vec(manifest).unwrap(),
                ),
                ("networks.json".to_string(), networks.to_vec()),
            ]),
            requested: Mutex::default(),
        })
    }

    fn requested(&self) -> Vec<String> {
        self.requested.lock().unwrap().clone()
    }
}

fn networks_entry(version: &str, content: &[u8], min_app_version: &str) -> ContentEntry {
    ContentEntry {
        version: version.to_string(),
        path: "networks.json".to_string(),
        checksum: compute_checksum(content),
        size_bytes: content.len() as u64,
        min_app_version: min_app_version.to_string(),
        max_app_version: None,
    }
}

fn signed(signer: &SigningKeyPair, content: ContentIndex) -> ContentManifest {
    let mut manifest = ContentManifest {
        schema_version: 1,
        generated_at: "2026-10-27T00:00:00Z".to_string(),
        base_url: "https://cdn.vauchi.app/v1/".to_string(),
        content,
        signature: None,
    };
    let canonical = serde_json::to_vec(&serde_json::to_value(&manifest).unwrap()).unwrap();
    manifest.signature = Some(hex::encode(signer.sign(&canonical).as_bytes()));
    manifest
}

fn networks_manifest(signer: &SigningKeyPair, version: &str) -> ContentManifest {
    signed(
        signer,
        ContentIndex {
            networks: Some(networks_entry(version, NETWORKS, "0.1.0")),
            ..ContentIndex::default()
        },
    )
}

fn manager(
    dir: &Path,
    publisher: &SigningKeyPair,
    source: Arc<RecordingSource>,
    now: SystemTime,
) -> ContentManager {
    let config = ContentConfig {
        storage_path: dir.to_path_buf(),
        publisher_public_key: publisher.public_key(),
        check_interval: Duration::from_secs(3600),
        source: Some(ContentSourceHandle(source)),
        ..Default::default()
    };
    ContentManager::new(config, Arc::new(PinnedClock(now))).unwrap()
}

fn launch_time() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

// @scenario: remote_content :: Respect update check interval
#[test]
fn a_cycle_inside_the_check_interval_fetches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let publisher = SigningKeyPair::generate();
    ContentCache::new(dir.path())
        .unwrap()
        .set_last_check_time(launch_time() - Duration::from_secs(30 * 60))
        .unwrap();
    let source = RecordingSource::serving(&networks_manifest(&publisher, "1.1.0"), NETWORKS);
    let manager = manager(dir.path(), &publisher, Arc::clone(&source), launch_time());

    let (status, applied) = manager.run_update_cycle_blocking();

    assert!(matches!(status, UpdateStatus::UpToDate), "got {status:?}");
    assert!(applied.is_none());
    assert_eq!(source.requested(), Vec::<String>::new());
}

// @scenario: remote_content :: Check for updates when interval elapsed
#[test]
fn a_cycle_after_the_check_interval_fetches_the_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let publisher = SigningKeyPair::generate();
    ContentCache::new(dir.path())
        .unwrap()
        .set_last_check_time(launch_time() - Duration::from_secs(2 * 3600))
        .unwrap();
    let source = RecordingSource::serving(&networks_manifest(&publisher, "1.1.0"), NETWORKS);
    let manager = manager(dir.path(), &publisher, Arc::clone(&source), launch_time());

    let (status, _) = manager.run_update_cycle_blocking();

    assert!(
        matches!(&status, UpdateStatus::UpdatesAvailable(types) if types == &[ContentType::Networks]),
        "got {status:?}"
    );
    assert_eq!(
        source.requested().first().map(String::as_str),
        Some("manifest.json")
    );
    assert_eq!(manager.cache().get_last_check_time(), Some(launch_time()));
}

// @scenario: remote_content :: Detect available updates
#[test]
fn a_newer_remote_version_is_reported_as_an_update() {
    let dir = tempfile::tempdir().unwrap();
    let publisher = SigningKeyPair::generate();
    ContentCache::new(dir.path())
        .unwrap()
        .save_manifest(&networks_manifest(&publisher, "1.0.0"))
        .unwrap();
    let source = RecordingSource::serving(&networks_manifest(&publisher, "1.1.0"), NETWORKS);
    let manager = manager(dir.path(), &publisher, source, launch_time());

    let (status, _) = manager.run_update_cycle_blocking();

    assert!(
        matches!(&status, UpdateStatus::UpdatesAvailable(types) if types == &[ContentType::Networks]),
        "got {status:?}"
    );
}

// @scenario: remote_content :: No updates when versions match
#[test]
fn a_matching_remote_version_is_up_to_date_and_downloads_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let publisher = SigningKeyPair::generate();
    ContentCache::new(dir.path())
        .unwrap()
        .save_manifest(&networks_manifest(&publisher, "1.0.0"))
        .unwrap();
    let source = RecordingSource::serving(&networks_manifest(&publisher, "1.0.0"), NETWORKS);
    let manager = manager(dir.path(), &publisher, Arc::clone(&source), launch_time());

    let (status, applied) = manager.run_update_cycle_blocking();

    assert!(matches!(status, UpdateStatus::UpToDate), "got {status:?}");
    assert!(applied.is_none());
    assert_eq!(source.requested(), vec!["manifest.json".to_string()]);
}

// @scenario: remote_content :: Reject a manifest signed by an untrusted publisher
#[test]
fn a_manifest_signed_by_another_key_is_rejected_and_nothing_is_applied() {
    let dir = tempfile::tempdir().unwrap();
    let trusted = SigningKeyPair::generate();
    let untrusted = SigningKeyPair::generate();
    let source = RecordingSource::serving(&networks_manifest(&untrusted, "1.1.0"), NETWORKS);
    let manager = manager(dir.path(), &trusted, Arc::clone(&source), launch_time());

    let (status, applied) = manager.run_update_cycle_blocking();

    assert!(
        matches!(&status, UpdateStatus::CheckFailed(message) if message.contains("Signature verification failed")),
        "got {status:?}"
    );
    assert!(applied.is_none());
    assert_eq!(source.requested(), vec!["manifest.json".to_string()]);
    assert!(manager.cache().get_manifest().is_none());
    assert!(
        manager
            .cache()
            .get_content(ContentType::Networks, "networks.json")
            .is_none()
    );
}

// @scenario: remote_content :: Verify content checksum before saving
#[test]
fn content_matching_its_checksum_is_saved_to_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let publisher = SigningKeyPair::generate();
    let source = RecordingSource::serving(&networks_manifest(&publisher, "1.1.0"), NETWORKS);
    let manager = manager(dir.path(), &publisher, source, launch_time());

    let (_, applied) = manager.run_update_cycle_blocking();

    assert!(
        matches!(
            &applied,
            Some(Ok(ApplyResult::Applied { applied, failed }))
                if applied == &[ContentType::Networks] && failed.is_empty()
        ),
        "got {applied:?}"
    );
    assert_eq!(
        manager
            .cache()
            .get_content(ContentType::Networks, "networks.json"),
        Some(NETWORKS.to_vec())
    );
}

// @scenario: remote_content :: Reject content with mismatched checksum
#[test]
fn content_failing_its_checksum_is_not_saved_and_the_cached_copy_stays() {
    let dir = tempfile::tempdir().unwrap();
    let publisher = SigningKeyPair::generate();
    let cached_networks = br#"{"networks":["cached"]}"#;
    let cache = ContentCache::new(dir.path()).unwrap();
    cache
        .save_content(
            ContentType::Networks,
            "networks.json",
            cached_networks,
            &compute_checksum(cached_networks),
        )
        .unwrap();
    cache
        .save_manifest(&networks_manifest(&publisher, "1.0.0"))
        .unwrap();
    let source = RecordingSource::serving(
        &networks_manifest(&publisher, "1.1.0"),
        br#"{"networks":["tampered"]}"#,
    );
    let manager = manager(dir.path(), &publisher, source, launch_time());

    let (_, applied) = manager.run_update_cycle_blocking();

    assert!(
        matches!(
            &applied,
            Some(Ok(ApplyResult::Applied { applied, failed }))
                if applied.is_empty()
                    && failed.len() == 1
                    && failed[0].0 == ContentType::Networks
                    && failed[0].1.contains("Checksum mismatch")
        ),
        "got {applied:?}"
    );
    assert_eq!(
        manager
            .cache()
            .get_content(ContentType::Networks, "networks.json"),
        Some(cached_networks.to_vec())
    );
    assert_eq!(
        manager
            .cache()
            .get_manifest()
            .and_then(|manifest| manifest.content.networks)
            .map(|entry| entry.version),
        Some("1.0.0".to_string())
    );
}

// @scenario: remote_content :: Apply updates compatible with current app version
#[test]
fn only_content_this_app_version_supports_is_listed_as_an_update() {
    let dir = tempfile::tempdir().unwrap();
    let publisher = SigningKeyPair::generate();
    let remote = signed(
        &publisher,
        ContentIndex {
            networks: Some(networks_entry("2.0.0", NETWORKS, "999.0.0")),
            locales: Some(LocalesEntry {
                version: "1.5.0".to_string(),
                path: "locales/".to_string(),
                files: HashMap::new(),
                min_app_version: "0.1.0".to_string(),
            }),
            ..ContentIndex::default()
        },
    );
    let source = RecordingSource::serving(&remote, NETWORKS);
    let manager = manager(dir.path(), &publisher, source, launch_time());

    let (status, _) = manager.run_update_cycle_blocking();

    assert!(
        matches!(&status, UpdateStatus::UpdatesAvailable(types) if types == &[ContentType::Locales]),
        "got {status:?}"
    );
}
