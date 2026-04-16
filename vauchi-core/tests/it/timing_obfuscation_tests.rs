// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for timing obfuscation configuration (C1-C3).

use std::time::Duration;

use vauchi_core::api::SyncConfig;

// --- C1: Post-Exchange Sync Delay ---

#[test]
fn test_post_exchange_delay_in_range() {
    let config = SyncConfig {
        post_exchange_delay_min_ms: 30_000,
        post_exchange_delay_max_ms: 300_000,
        ..Default::default()
    };
    for _ in 0..100 {
        let delay = config.random_post_exchange_delay();
        assert!(
            delay >= Duration::from_secs(30),
            "delay {delay:?} below 30s minimum"
        );
        assert!(
            delay <= Duration::from_secs(300),
            "delay {delay:?} above 300s maximum"
        );
    }
}

#[test]
fn test_post_exchange_delay_min_equals_max() {
    let config = SyncConfig {
        post_exchange_delay_min_ms: 60_000,
        post_exchange_delay_max_ms: 60_000,
        ..Default::default()
    };
    let delay = config.random_post_exchange_delay();
    assert_eq!(delay, Duration::from_secs(60));
}

#[test]
fn test_post_exchange_delay_min_greater_than_max() {
    let config = SyncConfig {
        post_exchange_delay_min_ms: 120_000,
        post_exchange_delay_max_ms: 60_000,
        ..Default::default()
    };
    let delay = config.random_post_exchange_delay();
    assert_eq!(
        delay,
        Duration::from_millis(120_000),
        "when min > max, should return min"
    );
}

// --- C2: Sync Interval Jitter ---

#[test]
fn test_jittered_sync_interval_in_range() {
    let config = SyncConfig {
        sync_interval_ms: 60_000,
        sync_interval_jitter_percent: 15,
        ..Default::default()
    };
    for _ in 0..100 {
        let interval = config.jittered_sync_interval();
        assert!(
            interval >= Duration::from_millis(51_000),
            "interval {interval:?} below 51s (60s - 15%)"
        );
        assert!(
            interval <= Duration::from_millis(69_000),
            "interval {interval:?} above 69s (60s + 15%)"
        );
    }
}

#[test]
fn test_zero_jitter_returns_exact_interval() {
    let config = SyncConfig {
        sync_interval_ms: 60_000,
        sync_interval_jitter_percent: 0,
        ..Default::default()
    };
    let interval = config.jittered_sync_interval();
    assert_eq!(interval, Duration::from_secs(60));
}

#[test]
fn test_jitter_capped_at_50_percent() {
    let config = SyncConfig {
        sync_interval_ms: 60_000,
        sync_interval_jitter_percent: 100, // should be capped to 50
        ..Default::default()
    };
    for _ in 0..100 {
        let interval = config.jittered_sync_interval();
        assert!(
            interval >= Duration::from_millis(30_000),
            "interval {interval:?} below 30s (60s - 50%)"
        );
        assert!(
            interval <= Duration::from_millis(90_000),
            "interval {interval:?} above 90s (60s + 50%)"
        );
    }
}

// --- C3: Padding Config ---

#[test]
fn test_padding_config_defaults_to_enabled() {
    let config = SyncConfig::default();
    assert!(config.padding_enabled);
}

// --- Default values ---

#[test]
fn test_sync_config_timing_defaults() {
    let config = SyncConfig::default();
    assert_eq!(config.post_exchange_delay_min_ms, 30_000);
    assert_eq!(config.post_exchange_delay_max_ms, 300_000);
    assert_eq!(config.sync_interval_jitter_percent, 15);
}
