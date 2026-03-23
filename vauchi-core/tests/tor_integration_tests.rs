// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tor integration tests (feature-gated, some require network).
//!
//! Tests marked `#[ignore]` require a live network connection and
//! take 10-30 seconds to bootstrap the Tor network. Run manually:
//!
//! ```bash
//! cargo test --features tor -p vauchi-core --test tor_integration_tests -- --ignored
//! ```

#![cfg(feature = "tor")]

use vauchi_core::network::tor::{TorConnector, TorManager, TorStatus};
use vauchi_core::tor_config::TorConfig;

// ============================================================
// Compilation verification (always runs with --features tor)
// ============================================================

#[test]
fn test_tor_manager_creates_successfully() {
    let config = TorConfig::default();
    let manager = TorManager::new(config);
    assert!(manager.is_ok(), "TorManager::new should succeed");
}

#[test]
fn test_tor_manager_initial_status_is_disabled() {
    let manager = TorManager::new(TorConfig::default()).unwrap();
    assert_eq!(manager.status(), TorStatus::Disabled);
}

#[test]
fn test_tor_manager_circuit_age_none_before_bootstrap() {
    let manager = TorManager::new(TorConfig::default()).unwrap();
    assert!(manager.circuit_age_secs().is_none());
    assert!(!manager.needs_circuit_rotation());
}

#[test]
fn test_tor_manager_config_preserved() {
    let config = TorConfig::enabled()
        .with_prefer_onion(false)
        .with_circuit_rotation_secs(300);
    let manager = TorManager::new(config).unwrap();
    assert!(manager.config().enabled);
    assert!(!manager.config().prefer_onion);
    assert_eq!(manager.config().circuit_rotation_secs, 300);
}

#[test]
fn test_tor_manager_connect_before_bootstrap_fails() {
    let manager = TorManager::new(TorConfig::default()).unwrap();
    let err = manager.connect_to("example.com", 443).err();
    assert!(err.is_some(), "connect before bootstrap should fail");
    assert!(
        matches!(
            err.unwrap(),
            vauchi_core::network::NetworkError::TorNotAvailable
        ),
        "expected TorNotAvailable error variant"
    );
}

#[test]
fn test_tor_manager_rotate_before_bootstrap_fails() {
    let manager = TorManager::new(TorConfig::default()).unwrap();
    let result = manager.rotate_circuit();
    assert!(result.is_err(), "rotate before bootstrap should fail");
    assert!(
        matches!(
            result.unwrap_err(),
            vauchi_core::network::NetworkError::TorNotAvailable
        ),
        "expected TorNotAvailable error variant"
    );
}

#[test]
fn test_tor_manager_shutdown_before_bootstrap_succeeds() {
    let manager = TorManager::new(TorConfig::default()).unwrap();
    // Shutdown on an un-bootstrapped manager should succeed (no-op)
    manager
        .shutdown()
        .expect("shutdown before bootstrap should succeed");
    assert_eq!(manager.status(), TorStatus::Disabled);
}

// ============================================================
// Live network tests (require --ignored flag)
// ============================================================

/// Bootstrap the Tor network and verify connection status.
///
/// This test takes 10-30 seconds and requires network access.
/// Run: cargo test --features tor -p vauchi-core --test tor_integration_tests -- --ignored
#[test]
#[ignore]
fn test_tor_live_bootstrap() {
    let manager = TorManager::new(TorConfig::default()).unwrap();

    manager
        .bootstrap()
        .expect("Tor bootstrap should succeed (requires network)");
    assert_eq!(manager.status(), TorStatus::Connected);
    let age = manager
        .circuit_age_secs()
        .expect("circuit age should be available after bootstrap");
    assert!(age < 60, "fresh circuit age should be under 60s, got {age}");

    // Shutdown
    manager.shutdown().unwrap();
    assert_eq!(manager.status(), TorStatus::Disabled);
}

/// Bootstrap and connect to a known host through Tor.
#[test]
#[ignore]
fn test_tor_live_connect() {
    let manager = TorManager::new(TorConfig::default()).unwrap();
    manager.bootstrap().unwrap();

    let result = manager.connect_to("example.com", 80);
    assert!(result.is_ok(), "Tor connect failed: {:?}", result.err());

    manager.shutdown().unwrap();
}

/// Bootstrap and rotate circuit (now uses isolated_client).
#[test]
#[ignore]
fn test_tor_live_circuit_rotation() {
    let manager = TorManager::new(TorConfig::default()).unwrap();
    manager.bootstrap().unwrap();

    let result = manager.rotate_circuit();
    assert!(
        result.is_ok(),
        "Circuit rotation failed: {:?}",
        result.err()
    );
    assert_eq!(manager.status(), TorStatus::Connected);
    // Circuit age should be reset (very recent)
    assert!(manager.circuit_age_secs().unwrap() < 2);

    manager.shutdown().unwrap();
}
