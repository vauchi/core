//! Tests for content cache
//!
//! Scenarios from remote-content.feature:
//! - Atomic cache writes
//! - Use cached manifest when offline
//! - Content resolution order

use tempfile::TempDir;
use vauchi_core::content::{
    compute_checksum, ContentCache, ContentEntry, ContentIndex, ContentManifest, ContentType,
};

fn create_test_manifest() -> ContentManifest {
    ContentManifest {
        schema_version: 1,
        generated_at: "2026-01-24T12:00:00Z".to_string(),
        base_url: "https://vauchi.app/app-files".to_string(),
        content: ContentIndex {
            networks: Some(ContentEntry {
                version: "1.0.0".to_string(),
                path: "networks/v1/networks.json".to_string(),
                checksum: "sha256:abc123".to_string(),
                size_bytes: 1024,
                min_app_version: "1.0.0".to_string(),
                max_app_version: None,
            }),
            locales: None,
            help: None,
            themes: None,
        },
    }
}

#[test]
fn test_cache_new_creates_directory() {
    let temp = TempDir::new().unwrap();
    let cache = ContentCache::new(temp.path()).unwrap();

    assert!(temp.path().join("content").exists());
    drop(cache);
}

#[test]
fn test_cache_manifest_roundtrip() {
    let temp = TempDir::new().unwrap();
    let cache = ContentCache::new(temp.path()).unwrap();

    let manifest = create_test_manifest();
    cache.save_manifest(&manifest).unwrap();

    let loaded = cache.get_manifest().unwrap();
    assert_eq!(loaded.schema_version, 1);
    assert_eq!(loaded.base_url, "https://vauchi.app/app-files");
    assert!(loaded.content.networks.is_some());
}

#[test]
fn test_cache_manifest_not_found() {
    let temp = TempDir::new().unwrap();
    let cache = ContentCache::new(temp.path()).unwrap();

    assert!(cache.get_manifest().is_none());
}

#[test]
fn test_cache_content_roundtrip() {
    let temp = TempDir::new().unwrap();
    let cache = ContentCache::new(temp.path()).unwrap();

    let data = b"test content data";
    let checksum = compute_checksum(data);

    cache
        .save_content(ContentType::Networks, "networks.json", data, &checksum)
        .unwrap();

    let loaded = cache
        .get_content(ContentType::Networks, "networks.json")
        .unwrap();
    assert_eq!(loaded, data);
}

#[test]
fn test_cache_content_creates_subdirectory() {
    let temp = TempDir::new().unwrap();
    let cache = ContentCache::new(temp.path()).unwrap();

    let data = b"theme data";
    let checksum = compute_checksum(data);

    cache
        .save_content(ContentType::Themes, "dark.json", data, &checksum)
        .unwrap();

    assert!(temp.path().join("content/themes/dark.json").exists());
}

#[test]
fn test_cache_content_not_found() {
    let temp = TempDir::new().unwrap();
    let cache = ContentCache::new(temp.path()).unwrap();

    assert!(cache
        .get_content(ContentType::Networks, "nonexistent.json")
        .is_none());
}

#[test]
fn test_cache_rejects_invalid_checksum() {
    let temp = TempDir::new().unwrap();
    let cache = ContentCache::new(temp.path()).unwrap();

    let data = b"test content";
    let wrong_checksum = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

    let result = cache.save_content(ContentType::Networks, "networks.json", data, wrong_checksum);
    assert!(result.is_err());

    // File should not exist
    assert!(cache
        .get_content(ContentType::Networks, "networks.json")
        .is_none());
}

#[test]
fn test_cache_atomic_write_no_partial_files() {
    let temp = TempDir::new().unwrap();
    let cache = ContentCache::new(temp.path()).unwrap();

    let data = b"test content";
    let checksum = compute_checksum(data);

    cache
        .save_content(ContentType::Networks, "test.json", data, &checksum)
        .unwrap();

    // No .tmp files should remain
    let tmp_path = temp.path().join("content/networks/test.json.tmp");
    assert!(!tmp_path.exists());
}

#[test]
fn test_cache_overwrites_existing_content() {
    let temp = TempDir::new().unwrap();
    let cache = ContentCache::new(temp.path()).unwrap();

    let data1 = b"version 1";
    let checksum1 = compute_checksum(data1);
    cache
        .save_content(ContentType::Networks, "networks.json", data1, &checksum1)
        .unwrap();

    let data2 = b"version 2";
    let checksum2 = compute_checksum(data2);
    cache
        .save_content(ContentType::Networks, "networks.json", data2, &checksum2)
        .unwrap();

    let loaded = cache
        .get_content(ContentType::Networks, "networks.json")
        .unwrap();
    assert_eq!(loaded, b"version 2");
}

#[test]
fn test_cache_multiple_content_types() {
    let temp = TempDir::new().unwrap();
    let cache = ContentCache::new(temp.path()).unwrap();

    let networks = b"networks data";
    let themes = b"themes data";
    let locales = b"locales data";

    cache
        .save_content(
            ContentType::Networks,
            "networks.json",
            networks,
            &compute_checksum(networks),
        )
        .unwrap();
    cache
        .save_content(
            ContentType::Themes,
            "dark.json",
            themes,
            &compute_checksum(themes),
        )
        .unwrap();
    cache
        .save_content(
            ContentType::Locales,
            "en.json",
            locales,
            &compute_checksum(locales),
        )
        .unwrap();

    assert_eq!(
        cache
            .get_content(ContentType::Networks, "networks.json")
            .unwrap(),
        networks
    );
    assert_eq!(
        cache.get_content(ContentType::Themes, "dark.json").unwrap(),
        themes
    );
    assert_eq!(
        cache.get_content(ContentType::Locales, "en.json").unwrap(),
        locales
    );
}

#[test]
fn test_cache_clear_content_type() {
    let temp = TempDir::new().unwrap();
    let cache = ContentCache::new(temp.path()).unwrap();

    let data = b"test data";
    cache
        .save_content(
            ContentType::Networks,
            "networks.json",
            data,
            &compute_checksum(data),
        )
        .unwrap();

    cache.clear_content_type(ContentType::Networks).unwrap();

    assert!(cache
        .get_content(ContentType::Networks, "networks.json")
        .is_none());
}

#[test]
fn test_cache_last_check_time() {
    let temp = TempDir::new().unwrap();
    let cache = ContentCache::new(temp.path()).unwrap();

    // Initially no last check time
    assert!(cache.get_last_check_time().is_none());

    // Set last check time
    let now = std::time::SystemTime::now();
    cache.set_last_check_time(now).unwrap();

    let loaded = cache.get_last_check_time().unwrap();
    // Allow 1 second tolerance for serialization roundtrip
    let diff = now.duration_since(loaded).unwrap_or_default();
    assert!(diff.as_secs() < 1);
}
