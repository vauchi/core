// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for mesh exchange module (Phase 2A).
//!
//! Feature: mesh_exchange.feature @ble @mesh

use std::time::Duration;

use vauchi_core::exchange::mesh::{
    DiscoveredPeer, MeshAdvertisement, MeshConfig, MeshExchangeManager, MeshState,
};
use vauchi_core::exchange::{MockBLETransport, MockProximityVerifier};

// --- MeshAdvertisement tests ---

#[test]
fn test_mesh_advertisement_contains_service_uuid_and_session_id() {
    let ad = MeshAdvertisement::new();
    assert_eq!(
        ad.service_uuid(),
        vauchi_core::exchange::VAUCHI_BLE_SERVICE_UUID
    );
    assert_ne!(ad.session_id(), &[0u8; 16]);
}

#[test]
fn test_mesh_advertisement_has_no_identity_info() {
    let ad = MeshAdvertisement::new();
    // Privacy: advertisement must not contain name, public key, or any identifying info
    assert!(ad.name().is_none());
    assert!(ad.public_key().is_none());
}

#[test]
fn test_mesh_advertisement_session_id_randomized() {
    let ad1 = MeshAdvertisement::new();
    let ad2 = MeshAdvertisement::new();
    // Two advertisements should have different session IDs (probabilistically)
    assert_ne!(ad1.session_id(), ad2.session_id());
}

#[test]
fn test_mesh_advertisement_serialization_roundtrip() {
    let ad = MeshAdvertisement::new();
    let bytes = ad.to_bytes();
    let parsed = MeshAdvertisement::from_bytes(&bytes).unwrap();
    assert_eq!(ad.session_id(), parsed.session_id());
}

// --- MeshConfig tests ---

#[test]
fn test_mesh_config_defaults() {
    let config = MeshConfig::default();
    assert_eq!(config.scan_interval, Duration::from_secs(5));
    assert_eq!(config.session_timeout, Duration::from_secs(30));
    assert_eq!(config.max_concurrent_discoveries, 10);
}

// --- MeshState tests ---

#[test]
fn test_mesh_state_initial_is_disabled() {
    let transport = MockBLETransport::with_peer_payload(&[0; 174]);
    let verifier = MockProximityVerifier::success();
    let config = MeshConfig::default();
    let manager = MeshExchangeManager::new(transport, verifier, config);
    assert!(matches!(manager.state(), MeshState::Disabled));
}

#[test]
fn test_mesh_state_transitions_to_active() {
    let transport = MockBLETransport::with_peer_payload(&[0; 174]);
    let verifier = MockProximityVerifier::success();
    let config = MeshConfig::default();
    let mut manager = MeshExchangeManager::new(transport, verifier, config);

    manager.enable().unwrap();
    assert!(matches!(manager.state(), MeshState::Active { .. }));
}

#[test]
fn test_mesh_state_disable_returns_to_disabled() {
    let transport = MockBLETransport::with_peer_payload(&[0; 174]);
    let verifier = MockProximityVerifier::success();
    let config = MeshConfig::default();
    let mut manager = MeshExchangeManager::new(transport, verifier, config);

    manager.enable().unwrap();
    manager.disable();
    assert!(matches!(manager.state(), MeshState::Disabled));
}

// --- Replay prevention tests ---

#[test]
fn test_completed_peer_prevents_replay() {
    let transport = MockBLETransport::with_peer_payload(&[0; 174]);
    let verifier = MockProximityVerifier::success();
    let config = MeshConfig::default();
    let mut manager = MeshExchangeManager::new(transport, verifier, config);

    manager.enable().unwrap();

    let peer_key = [42u8; 32];
    assert!(!manager.is_peer_completed(&peer_key));

    manager.mark_peer_completed(peer_key);
    assert!(manager.is_peer_completed(&peer_key));
}

// --- Session ID rotation tests ---

#[test]
fn test_session_id_rotates_on_reenable() {
    let transport = MockBLETransport::with_peer_payload(&[0; 174]);
    let verifier = MockProximityVerifier::success();
    let config = MeshConfig::default();
    let mut manager = MeshExchangeManager::new(transport, verifier, config);

    manager.enable().unwrap();
    let first_session = manager.current_session_id().unwrap();

    manager.disable();
    manager.enable().unwrap();
    let second_session = manager.current_session_id().unwrap();

    // Session ID should change on re-enable (privacy: prevents tracking)
    assert_ne!(first_session, second_session);
}

// --- DiscoveredPeer tests ---

#[test]
fn test_discovered_peer_creation() {
    let peer = DiscoveredPeer::new("device-1", [1u8; 16], -55);
    assert_eq!(peer.device_id(), "device-1");
    assert_eq!(peer.session_id(), &[1u8; 16]);
    assert_eq!(peer.rssi(), -55);
}

#[test]
fn test_discovered_peer_update_rssi() {
    let mut peer = DiscoveredPeer::new("device-1", [1u8; 16], -55);
    peer.update_rssi(-40);
    assert_eq!(peer.rssi(), -40);
}

// --- Discovery management tests ---

#[test]
fn test_add_discovered_peer() {
    let transport = MockBLETransport::with_peer_payload(&[0; 174]);
    let verifier = MockProximityVerifier::success();
    let config = MeshConfig::default();
    let mut manager = MeshExchangeManager::new(transport, verifier, config);

    manager.enable().unwrap();

    let peer = DiscoveredPeer::new("device-1", [1u8; 16], -50);
    manager.add_discovered_peer(peer);

    assert_eq!(manager.discovered_peers().len(), 1);
    assert_eq!(manager.discovered_peers()[0].device_id(), "device-1");
}

#[test]
fn test_max_concurrent_discoveries_enforced() {
    let transport = MockBLETransport::with_peer_payload(&[0; 174]);
    let verifier = MockProximityVerifier::success();
    let mut config = MeshConfig::default();
    config.max_concurrent_discoveries = 2;
    let mut manager = MeshExchangeManager::new(transport, verifier, config);

    manager.enable().unwrap();

    manager.add_discovered_peer(DiscoveredPeer::new("d1", [1u8; 16], -50));
    manager.add_discovered_peer(DiscoveredPeer::new("d2", [2u8; 16], -60));
    manager.add_discovered_peer(DiscoveredPeer::new("d3", [3u8; 16], -70));

    // Should cap at max_concurrent_discoveries
    assert_eq!(manager.discovered_peers().len(), 2);
}
