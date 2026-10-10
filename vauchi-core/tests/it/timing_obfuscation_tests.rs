// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for timing obfuscation configuration (C1-C3).

use std::time::Duration;

use vauchi_core::api::SyncConfig;

// --- C1: Post-Exchange Sync Delay ---

// @internal
#[test]
fn test_post_exchange_delay_in_range() {
    let config = SyncConfig {
        post_exchange_delay_min_ms: 30_000,
        post_exchange_delay_max_ms: 300_000,
        ..Default::default()
    };
    for _ in 0..100 {
        let delay = config.random_post_exchange_delay(&vauchi_core::rng::OsSecureRng::new());
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

// @internal
#[test]
fn test_post_exchange_delay_min_equals_max() {
    let config = SyncConfig {
        post_exchange_delay_min_ms: 60_000,
        post_exchange_delay_max_ms: 60_000,
        ..Default::default()
    };
    let delay = config.random_post_exchange_delay(&vauchi_core::rng::OsSecureRng::new());
    assert_eq!(delay, Duration::from_secs(60));
}

// @internal
#[test]
fn test_post_exchange_delay_min_greater_than_max() {
    let config = SyncConfig {
        post_exchange_delay_min_ms: 120_000,
        post_exchange_delay_max_ms: 60_000,
        ..Default::default()
    };
    let delay = config.random_post_exchange_delay(&vauchi_core::rng::OsSecureRng::new());
    assert_eq!(
        delay,
        Duration::from_millis(120_000),
        "when min > max, should return min"
    );
}

// --- C2: Sync interval schedule (private#603) ---
//
// Owner decision 2026-10-10: gaps between polls are exponential with a
// 60 s mean, clamped to 10 s-5 min. A fixed period (60 s +/-15%) is a
// timing signature a hop can recognise; memoryless gaps are not.

fn sample_intervals(config: &SyncConfig, n: usize) -> Vec<f64> {
    let rng = vauchi_core::rng::OsSecureRng::new();
    (0..n)
        .map(|_| config.next_sync_interval(&rng).as_secs_f64())
        .collect()
}

// @internal
#[test]
fn test_sync_intervals_stay_within_bounds() {
    let config = SyncConfig::default();
    for gap in sample_intervals(&config, 5_000) {
        assert!(
            (10.0..=300.0).contains(&gap),
            "gap {gap}s outside 10 s-5 min"
        );
    }
}

// @internal
#[test]
fn test_sync_intervals_are_exponential_around_a_60s_mean() {
    let gaps = sample_intervals(&SyncConfig::default(), 20_000);
    let n = gaps.len() as f64;
    let mean = gaps.iter().sum::<f64>() / n;
    // Clamping lifts the mean of Exp(60 s) to about 60.4 s.
    assert!((57.0..=64.0).contains(&mean), "mean {mean}s");
    // P(X < 60 s) = 1 - 1/e = 0.632 for an exponential; a 60 s +/-15%
    // uniform schedule gives 0.5, so this separates the two.
    let below_mean = gaps.iter().filter(|g| **g < 60.0).count() as f64 / n;
    assert!(
        (0.60..=0.66).contains(&below_mean),
        "P(gap < 60 s) = {below_mean}"
    );
    // P(X <= 10 s) = 1 - e^(-1/6) = 0.154, all clamped to the floor.
    let at_floor = gaps.iter().filter(|g| **g <= 10.0).count() as f64 / n;
    assert!(
        (0.13..=0.18).contains(&at_floor),
        "P(gap = 10 s) = {at_floor}"
    );
}

// @internal
#[test]
fn test_equal_bounds_pin_the_interval() {
    let config = SyncConfig {
        sync_interval_ms: 60_000,
        sync_interval_min_ms: 1_000,
        sync_interval_max_ms: 1_000,
        ..Default::default()
    };
    for gap in sample_intervals(&config, 100) {
        assert_eq!(gap, 1.0);
    }
}

// @internal
#[test]
fn test_zero_interval_means_manual_sync_only() {
    let config = SyncConfig {
        sync_interval_ms: 0,
        ..Default::default()
    };
    assert_eq!(
        config.next_sync_interval(&vauchi_core::rng::OsSecureRng::new()),
        Duration::ZERO
    );
}

// --- C3: Padding Config ---

// @internal
#[test]
fn test_padding_config_defaults_to_enabled() {
    let config = SyncConfig::default();
    assert!(config.padding_enabled);
}

// --- Default values ---

// @internal
#[test]
fn test_sync_config_timing_defaults() {
    let config = SyncConfig::default();
    assert_eq!(config.post_exchange_delay_min_ms, 30_000);
    assert_eq!(config.post_exchange_delay_max_ms, 300_000);
    assert_eq!(config.sync_interval_ms, 60_000);
    assert_eq!(config.sync_interval_min_ms, 10_000);
    assert_eq!(config.sync_interval_max_ms, 300_000);
}
