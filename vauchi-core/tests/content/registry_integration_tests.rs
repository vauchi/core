// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for SocialNetworkRegistry integration with ContentManager
//!
//! Scenarios from remote-content.feature:
//! - New social network appears after update
//! - Updated network URL template

use tempfile::TempDir;
use vauchi_core::content::{
    compute_checksum, ContentCache, ContentConfig, ContentManager, ContentType,
};
use vauchi_core::social::SocialNetworkRegistry;

fn test_config(temp: &TempDir) -> ContentConfig {
    ContentConfig {
        storage_path: temp.path().to_path_buf(),
        remote_updates_enabled: false,
        ..Default::default()
    }
}

#[test]
fn test_registry_from_content_manager_bundled() {
    let temp = TempDir::new().unwrap();
    let config = test_config(&temp);
    let content = ContentManager::new(config).unwrap();

    let registry = SocialNetworkRegistry::from_content_manager(&content);

    // Should have bundled networks
    assert!(!registry.is_empty());
    assert!(registry.get("twitter").is_some());
    assert!(registry.get("github").is_some());
}

#[test]
fn test_registry_from_content_manager_cached() {
    let temp = TempDir::new().unwrap();

    // Pre-populate cache with custom networks
    let cache = ContentCache::new(temp.path()).unwrap();
    let custom_networks = r#"[
        {"id": "custom1", "name": "Custom One", "url": "https://one.example.com/{username}"},
        {"id": "custom2", "name": "Custom Two", "url": "https://two.example.com/{username}"}
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

    let config = test_config(&temp);
    let content = ContentManager::new(config).unwrap();

    let registry = SocialNetworkRegistry::from_content_manager(&content);

    // Should have cached networks (not bundled)
    assert_eq!(registry.len(), 2);
    assert!(registry.get("custom1").is_some());
    assert!(registry.get("custom2").is_some());
    assert!(registry.get("twitter").is_none()); // Bundled networks not present
}

#[test]
fn test_registry_generates_urls() {
    let temp = TempDir::new().unwrap();
    let config = test_config(&temp);
    let content = ContentManager::new(config).unwrap();

    let registry = SocialNetworkRegistry::from_content_manager(&content);

    let url = registry.profile_url("twitter", "alice");
    assert!(url.is_some());
    assert!(url.unwrap().contains("alice"));
}

#[test]
fn test_new_network_after_cache_update() {
    let temp = TempDir::new().unwrap();

    // Initially use bundled (no cache)
    let config = test_config(&temp);
    let content1 = ContentManager::new(config.clone()).unwrap();
    let registry1 = SocialNetworkRegistry::from_content_manager(&content1);

    // "newnetwork" should not exist in bundled
    assert!(registry1.get("newnetwork").is_none());

    // Add new network to cache
    let cache = ContentCache::new(temp.path()).unwrap();
    let updated_networks = r#"[
        {"id": "twitter", "name": "Twitter / X", "url": "https://twitter.com/{username}"},
        {"id": "newnetwork", "name": "New Network", "url": "https://new.example.com/{username}"}
    ]"#;
    let checksum = compute_checksum(updated_networks.as_bytes());
    cache
        .save_content(
            ContentType::Networks,
            "networks.json",
            updated_networks.as_bytes(),
            &checksum,
        )
        .unwrap();

    // Create new manager to pick up cache
    let content2 = ContentManager::new(config).unwrap();
    let registry2 = SocialNetworkRegistry::from_content_manager(&content2);

    // Now "newnetwork" should exist
    assert!(registry2.get("newnetwork").is_some());
    assert_eq!(
        registry2.get("newnetwork").unwrap().display_name(),
        "New Network"
    );
}

#[test]
fn test_updated_url_template() {
    let temp = TempDir::new().unwrap();

    // Update twitter URL template in cache
    let cache = ContentCache::new(temp.path()).unwrap();
    let updated_networks = r#"[
        {"id": "twitter", "name": "X (formerly Twitter)", "url": "https://x.com/{username}"}
    ]"#;
    let checksum = compute_checksum(updated_networks.as_bytes());
    cache
        .save_content(
            ContentType::Networks,
            "networks.json",
            updated_networks.as_bytes(),
            &checksum,
        )
        .unwrap();

    let config = test_config(&temp);
    let content = ContentManager::new(config).unwrap();
    let registry = SocialNetworkRegistry::from_content_manager(&content);

    // Should use updated URL
    let url = registry.profile_url("twitter", "alice").unwrap();
    assert!(url.contains("x.com")); // New domain
    assert!(!url.contains("twitter.com")); // Not old domain
}
