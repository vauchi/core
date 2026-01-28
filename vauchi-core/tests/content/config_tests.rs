// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for content configuration
//!
//! Scenarios from remote-content.feature:
//! - Disable remote updates via settings
//! - Content fetched via Tor when configured

use std::path::PathBuf;
use std::time::Duration;
use vauchi_core::content::ContentConfig;

#[test]
fn test_config_default() {
    let config = ContentConfig::default();

    assert_eq!(config.content_url, "https://vauchi.app/app-files");
    assert!(config.remote_updates_enabled);
    assert_eq!(config.check_interval, Duration::from_secs(3600));
    assert_eq!(config.timeout, Duration::from_secs(30));
    assert_eq!(config.max_content_size, 5 * 1024 * 1024);
    assert!(config.proxy_url.is_none());
}

#[test]
fn test_config_with_storage_path() {
    let config = ContentConfig {
        storage_path: PathBuf::from("/tmp/vauchi"),
        ..Default::default()
    };

    assert_eq!(config.storage_path, PathBuf::from("/tmp/vauchi"));
}

#[test]
fn test_config_with_tor() {
    let config = ContentConfig::default().with_tor();

    assert_eq!(
        config.proxy_url,
        Some("socks5://127.0.0.1:9050".to_string())
    );
    // Longer timeout for Tor
    assert_eq!(config.timeout, Duration::from_secs(60));
}

#[test]
fn test_config_disable_remote_updates() {
    let config = ContentConfig {
        remote_updates_enabled: false,
        ..Default::default()
    };

    assert!(!config.remote_updates_enabled);
}

#[test]
fn test_config_custom_url() {
    let config = ContentConfig {
        content_url: "https://custom.example.com/content".to_string(),
        ..Default::default()
    };

    assert_eq!(config.content_url, "https://custom.example.com/content");
}

#[test]
fn test_config_custom_interval() {
    let config = ContentConfig {
        check_interval: Duration::from_secs(1800), // 30 minutes
        ..Default::default()
    };

    assert_eq!(config.check_interval, Duration::from_secs(1800));
}

#[test]
fn test_config_custom_timeout() {
    let config = ContentConfig {
        timeout: Duration::from_secs(15),
        ..Default::default()
    };

    assert_eq!(config.timeout, Duration::from_secs(15));
}

#[test]
fn test_config_custom_max_size() {
    let config = ContentConfig {
        max_content_size: 10 * 1024 * 1024, // 10 MB
        ..Default::default()
    };

    assert_eq!(config.max_content_size, 10 * 1024 * 1024);
}
