// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for content types
//!
//! Scenarios from remote-content.feature:
//! - Detect available updates
//! - No updates when versions match
//! - Skip updates requiring newer app version

use vauchi_core::content::{ContentEntry, ContentManifest, ContentType, UpdateStatus};

#[test]
fn test_content_manifest_deserialize() {
    let json = r#"{
        "schema_version": 1,
        "generated_at": "2026-01-24T12:00:00Z",
        "base_url": "https://vauchi.app/app-files",
        "content": {
            "networks": {
                "version": "1.0.0",
                "path": "networks/v1/networks.json",
                "checksum": "sha256:abc123",
                "size_bytes": 1024,
                "min_app_version": "1.0.0"
            }
        }
    }"#;

    let manifest: ContentManifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.base_url, "https://vauchi.app/app-files");
    assert!(manifest.content.networks.is_some());

    let networks = manifest.content.networks.unwrap();
    assert_eq!(networks.version, "1.0.0");
    assert_eq!(networks.checksum, "sha256:abc123");
}

#[test]
fn test_content_manifest_with_locales() {
    let json = r#"{
        "schema_version": 1,
        "generated_at": "2026-01-24T12:00:00Z",
        "base_url": "https://vauchi.app/app-files",
        "content": {
            "locales": {
                "version": "1.0.0",
                "path": "locales/v1/",
                "min_app_version": "1.0.0",
                "files": {
                    "en": {
                        "path": "en.json",
                        "checksum": "sha256:en123",
                        "size_bytes": 2048
                    },
                    "de": {
                        "path": "de.json",
                        "checksum": "sha256:de123",
                        "size_bytes": 2100
                    }
                }
            }
        }
    }"#;

    let manifest: ContentManifest = serde_json::from_str(json).unwrap();
    let locales = manifest.content.locales.unwrap();
    assert_eq!(locales.files.len(), 2);
    assert!(locales.files.contains_key("en"));
    assert!(locales.files.contains_key("de"));
}

#[test]
fn test_content_entry_max_version() {
    let json = r#"{
        "version": "2.0.0",
        "path": "networks/v2/networks.json",
        "checksum": "sha256:xyz789",
        "size_bytes": 2048,
        "min_app_version": "1.5.0",
        "max_app_version": "2.0.0"
    }"#;

    let entry: ContentEntry = serde_json::from_str(json).unwrap();
    assert_eq!(entry.max_app_version, Some("2.0.0".to_string()));
}

#[test]
fn test_content_type_dir_name() {
    assert_eq!(ContentType::Networks.dir_name(), "networks");
    assert_eq!(ContentType::Locales.dir_name(), "locales");
    assert_eq!(ContentType::Help.dir_name(), "help");
    assert_eq!(ContentType::Themes.dir_name(), "themes");
}

#[test]
fn test_update_status_variants() {
    let up_to_date = UpdateStatus::UpToDate;
    assert!(matches!(up_to_date, UpdateStatus::UpToDate));

    let updates = UpdateStatus::UpdatesAvailable(vec![ContentType::Networks, ContentType::Locales]);
    if let UpdateStatus::UpdatesAvailable(types) = updates {
        assert_eq!(types.len(), 2);
    } else {
        panic!("Expected UpdatesAvailable");
    }

    let failed = UpdateStatus::CheckFailed("Network error".to_string());
    if let UpdateStatus::CheckFailed(msg) = failed {
        assert_eq!(msg, "Network error");
    } else {
        panic!("Expected CheckFailed");
    }

    let disabled = UpdateStatus::Disabled;
    assert!(matches!(disabled, UpdateStatus::Disabled));
}
