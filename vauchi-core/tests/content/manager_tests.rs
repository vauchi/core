// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for ContentManager
//!
//! Scenarios from remote-content.feature:
//! - Use bundled content when cache is empty and offline
//! - Content resolution order (cache → bundled)
//! - Check for updates when interval elapsed
//! - Disable remote updates via settings

use std::time::Duration;
use tempfile::TempDir;
use vauchi_core::content::{
    compute_checksum, ContentCache, ContentConfig, ContentManager, ContentType, UpdateStatus,
};

fn test_config(temp: &TempDir) -> ContentConfig {
    ContentConfig {
        storage_path: temp.path().to_path_buf(),
        remote_updates_enabled: false, // Disable for unit tests
        ..Default::default()
    }
}

#[test]
fn test_manager_new_creates_cache() {
    let temp = TempDir::new().unwrap();
    let config = test_config(&temp);
    let _manager = ContentManager::new(config).unwrap();

    assert!(temp.path().join("content").exists());
}

#[test]
fn test_manager_returns_bundled_networks_when_cache_empty() {
    let temp = TempDir::new().unwrap();
    let config = test_config(&temp);
    let manager = ContentManager::new(config).unwrap();

    let networks = manager.networks();
    // Should return bundled networks (not empty)
    assert!(!networks.is_empty());
}

#[test]
fn test_manager_returns_cached_networks_over_bundled() {
    let temp = TempDir::new().unwrap();
    let config = test_config(&temp);

    // Pre-populate cache with custom networks
    let cache = ContentCache::new(temp.path()).unwrap();
    let custom_networks = r#"[
        {"id": "custom", "name": "Custom Network", "url": "https://custom.example.com/{username}"}
    ]"#;
    let checksum = compute_checksum(custom_networks.as_bytes());
    cache
        .save_content(
            ContentType::Networks,
            "networks.json",
            custom_networks.as_bytes(),
            &checksum,
        )
        .unwrap();

    let manager = ContentManager::new(config).unwrap();
    let networks = manager.networks();

    // Should return cached networks (1 custom network)
    assert_eq!(networks.len(), 1);
    assert_eq!(networks[0].id, "custom");
}

#[test]
fn test_manager_update_check_disabled() {
    let temp = TempDir::new().unwrap();
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        remote_updates_enabled: false,
        ..Default::default()
    };
    let manager = ContentManager::new(config).unwrap();

    let status = manager.check_for_updates_sync();
    assert!(matches!(status, UpdateStatus::Disabled));
}

#[test]
fn test_manager_should_check_respects_interval() {
    let temp = TempDir::new().unwrap();
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        remote_updates_enabled: true,
        check_interval: Duration::from_secs(3600),
        ..Default::default()
    };

    // Set last check time to now
    let cache = ContentCache::new(temp.path()).unwrap();
    cache
        .set_last_check_time(std::time::SystemTime::now())
        .unwrap();

    let manager = ContentManager::new(config).unwrap();

    // Should not check (interval not elapsed)
    assert!(!manager.should_check_now());
}

#[test]
fn test_manager_should_check_when_never_checked() {
    let temp = TempDir::new().unwrap();
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        remote_updates_enabled: true,
        check_interval: Duration::from_secs(3600),
        ..Default::default()
    };
    let manager = ContentManager::new(config).unwrap();

    // Should check (never checked before)
    assert!(manager.should_check_now());
}

#[test]
fn test_manager_should_check_when_interval_elapsed() {
    let temp = TempDir::new().unwrap();
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        remote_updates_enabled: true,
        check_interval: Duration::from_secs(1), // 1 second interval
        ..Default::default()
    };

    // Set last check time to 2 seconds ago
    let cache = ContentCache::new(temp.path()).unwrap();
    let two_secs_ago = std::time::SystemTime::now() - Duration::from_secs(2);
    cache.set_last_check_time(two_secs_ago).unwrap();

    let manager = ContentManager::new(config).unwrap();

    // Should check (interval elapsed)
    assert!(manager.should_check_now());
}

#[test]
fn test_manager_get_locale_returns_bundled_english() {
    let temp = TempDir::new().unwrap();
    let config = test_config(&temp);
    let manager = ContentManager::new(config).unwrap();

    let locale = manager.locale("en");
    // English should always be available as bundled
    assert!(locale.is_some());
}

#[test]
fn test_manager_get_locale_unknown_returns_none() {
    let temp = TempDir::new().unwrap();
    let config = test_config(&temp);
    let manager = ContentManager::new(config).unwrap();

    let locale = manager.locale("xx"); // Unknown language
    assert!(locale.is_none());
}

#[test]
fn test_manager_help_returns_none_when_cache_empty() {
    let temp = TempDir::new().unwrap();
    let config = test_config(&temp);
    let manager = ContentManager::new(config).unwrap();

    // No bundled help content; should return None
    let help = manager.help("en");
    assert!(help.is_none());
}

#[test]
fn test_manager_help_returns_cached_content() {
    let temp = TempDir::new().unwrap();
    let config = test_config(&temp);

    // Pre-populate cache with help content
    let cache = ContentCache::new(temp.path()).unwrap();
    let help_data =
        br#"{"getting_started": "Welcome to Vauchi", "faq": "Frequently asked questions"}"#;
    let checksum = compute_checksum(help_data);
    cache
        .save_content(ContentType::Help, "en.json", help_data, &checksum)
        .unwrap();

    let manager = ContentManager::new(config).unwrap();
    let help = manager.help("en");

    assert!(help.is_some());
    let strings = help.unwrap();
    assert_eq!(strings.get("getting_started").unwrap(), "Welcome to Vauchi");
    assert_eq!(strings.get("faq").unwrap(), "Frequently asked questions");
}

#[test]
fn test_manager_help_unknown_language_returns_none() {
    let temp = TempDir::new().unwrap();
    let config = test_config(&temp);
    let manager = ContentManager::new(config).unwrap();

    let help = manager.help("zz");
    assert!(help.is_none());
}

#[test]
fn test_manager_themes_returns_default_when_cache_empty() {
    let temp = TempDir::new().unwrap();
    let config = test_config(&temp);
    let manager = ContentManager::new(config).unwrap();

    let themes = manager.themes();
    assert_eq!(themes.len(), 1, "Should return single default theme");
    assert_eq!(themes[0].id, "default-dark");
}

#[test]
fn test_manager_themes_returns_cached_themes() {
    let temp = TempDir::new().unwrap();
    let config = test_config(&temp);
    let manager = ContentManager::new(config).unwrap();

    // Populate cache with themes.json
    let themes_json = r##"[
        {"id":"test-a","name":"Test A","version":"1.0.0","mode":"dark","colors":{"bg-primary":"#1a1a2e","bg-secondary":"#16213e","bg-tertiary":"#0f3460","text-primary":"#eeeeee","text-secondary":"#a0a0a0","accent":"#4fc3f7","accent-dark":"#0288d1","success":"#4caf50","error":"#f44336","warning":"#ff9800","border":"#333333"}},
        {"id":"test-b","name":"Test B","version":"1.0.0","mode":"light","colors":{"bg-primary":"#ffffff","bg-secondary":"#f5f5f5","bg-tertiary":"#e0e0e0","text-primary":"#212121","text-secondary":"#757575","accent":"#1976d2","accent-dark":"#0d47a1","success":"#388e3c","error":"#d32f2f","warning":"#f57c00","border":"#e0e0e0"}}
    ]"##;
    let checksum = compute_checksum(themes_json.as_bytes());
    let cache = ContentCache::new(temp.path()).unwrap();
    cache
        .save_content(
            ContentType::Themes,
            "themes.json",
            themes_json.as_bytes(),
            &checksum,
        )
        .unwrap();

    let themes = manager.themes();
    assert_eq!(themes.len(), 2);
    assert_eq!(themes[0].id, "test-a");
    assert_eq!(themes[1].id, "test-b");
}

#[test]
fn test_manager_record_check_time() {
    let temp = TempDir::new().unwrap();
    let config = test_config(&temp);
    let manager = ContentManager::new(config).unwrap();

    // Record check time
    manager.record_check_time().unwrap();

    // Should not need to check again immediately
    assert!(!manager.should_check_now());
}
