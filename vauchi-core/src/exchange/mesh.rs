// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mesh Exchange Module
//!
//! BLE offline peer-to-peer exchange using existing BLE abstractions.
//! Manages multi-peer discovery and exchange in event/venue settings.
//!
//! Privacy guarantees:
//! - Advertisements contain only service UUID + random session ID (no name, no key)
//! - Session ID rotated on each enable (prevents cross-session tracking)
//! - Completed peer keys tracked to prevent replay

use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::ble::{BLETransport, VAUCHI_BLE_SERVICE_UUID};
use super::proximity::ProximityVerifier;

// ============================================================
// MeshAdvertisement
// ============================================================

/// Privacy-preserving BLE mesh advertisement.
///
/// Contains only a service UUID and random session ID — no name, public key,
/// or other identifying information. The real exchange happens after
/// GATT connection.
#[derive(Debug, Clone)]
pub struct MeshAdvertisement {
    session_id: [u8; 16],
}

impl MeshAdvertisement {
    /// Creates a new advertisement with a random session ID.
    pub fn new() -> Self {
        use ring::rand::{SecureRandom, SystemRandom};
        let rng = SystemRandom::new();
        let mut session_id = [0u8; 16];
        rng.fill(&mut session_id).expect("RNG failed");
        MeshAdvertisement { session_id }
    }

    /// Returns the BLE service UUID.
    pub fn service_uuid(&self) -> &str {
        VAUCHI_BLE_SERVICE_UUID
    }

    /// Returns the random session ID.
    pub fn session_id(&self) -> &[u8; 16] {
        &self.session_id
    }

    /// Returns None — mesh advertisements never contain names (privacy).
    pub fn name(&self) -> Option<&str> {
        None
    }

    /// Returns None — mesh advertisements never contain public keys (privacy).
    pub fn public_key(&self) -> Option<&[u8; 32]> {
        None
    }

    /// Serializes to bytes (16-byte session ID).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.session_id.to_vec()
    }

    /// Parses from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, super::ble::BLEError> {
        if bytes.len() < 16 {
            return Err(super::ble::BLEError::InvalidPayload(
                "Mesh advertisement too short".into(),
            ));
        }
        let mut session_id = [0u8; 16];
        session_id.copy_from_slice(&bytes[..16]);
        Ok(MeshAdvertisement { session_id })
    }
}

// ============================================================
// MeshConfig
// ============================================================

/// Configuration for mesh exchange.
#[derive(Debug, Clone)]
pub struct MeshConfig {
    /// Interval between BLE scan cycles.
    pub scan_interval: Duration,
    /// Timeout for a single exchange attempt.
    pub session_timeout: Duration,
    /// Maximum number of discovered peers tracked simultaneously.
    pub max_concurrent_discoveries: usize,
}

impl Default for MeshConfig {
    fn default() -> Self {
        MeshConfig {
            scan_interval: Duration::from_secs(5),
            session_timeout: Duration::from_secs(30),
            max_concurrent_discoveries: 10,
        }
    }
}

// ============================================================
// MeshState
// ============================================================

/// State of the mesh exchange system.
#[derive(Debug, Clone)]
pub enum MeshState {
    /// Mesh is not active.
    Disabled,
    /// Actively scanning and advertising.
    Active {
        /// Number of currently discovered peers.
        discovered_count: usize,
    },
    /// Currently exchanging with a specific peer.
    Exchanging {
        /// Device ID of the peer being exchanged with.
        peer_id: String,
    },
    /// Temporarily paused (e.g., app backgrounded).
    Paused,
}

// ============================================================
// DiscoveredPeer
// ============================================================

/// A peer discovered during mesh scanning.
#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    device_id: String,
    session_id: [u8; 16],
    rssi: i16,
    first_seen: u64,
    last_seen: u64,
}

impl DiscoveredPeer {
    /// Creates a new discovered peer.
    pub fn new(device_id: &str, session_id: [u8; 16], rssi: i16) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
        DiscoveredPeer {
            device_id: device_id.to_string(),
            session_id,
            rssi,
            first_seen: now,
            last_seen: now,
        }
    }

    /// Returns the device ID.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Returns the session ID.
    pub fn session_id(&self) -> &[u8; 16] {
        &self.session_id
    }

    /// Returns the last known RSSI.
    pub fn rssi(&self) -> i16 {
        self.rssi
    }

    /// Updates the RSSI and last-seen timestamp.
    pub fn update_rssi(&mut self, rssi: i16) {
        self.rssi = rssi;
        self.last_seen = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
    }

    /// Returns the first-seen timestamp.
    pub fn first_seen(&self) -> u64 {
        self.first_seen
    }

    /// Returns the last-seen timestamp.
    pub fn last_seen(&self) -> u64 {
        self.last_seen
    }
}

// ============================================================
// MeshExchangeManager
// ============================================================

/// Orchestrates mesh BLE discovery and multi-peer exchange.
///
/// Generic over BLE transport and proximity verifier for testability.
pub struct MeshExchangeManager<T: BLETransport, P: ProximityVerifier> {
    transport: T,
    verifier: P,
    config: MeshConfig,
    state: MeshState,
    current_advertisement: Option<MeshAdvertisement>,
    discovered_peers: Vec<DiscoveredPeer>,
    completed_peer_keys: HashSet<[u8; 32]>,
}

impl<T: BLETransport, P: ProximityVerifier> MeshExchangeManager<T, P> {
    /// Creates a new mesh exchange manager.
    pub fn new(transport: T, verifier: P, config: MeshConfig) -> Self {
        MeshExchangeManager {
            transport,
            verifier,
            config,
            state: MeshState::Disabled,
            current_advertisement: None,
            discovered_peers: Vec::new(),
            completed_peer_keys: HashSet::new(),
        }
    }

    /// Returns the current state.
    pub fn state(&self) -> &MeshState {
        &self.state
    }

    /// Enables mesh exchange — starts advertising and scanning.
    ///
    /// Generates a new random session ID (privacy: prevents correlation
    /// across enable/disable cycles).
    pub fn enable(&mut self) -> Result<(), super::ble::BLEError> {
        let ad = MeshAdvertisement::new();
        self.current_advertisement = Some(ad);
        self.discovered_peers.clear();
        self.state = MeshState::Active {
            discovered_count: 0,
        };
        self.transport.start_scanning()?;
        Ok(())
    }

    /// Disables mesh exchange — stops advertising and scanning.
    pub fn disable(&mut self) {
        self.transport.stop();
        self.current_advertisement = None;
        self.discovered_peers.clear();
        self.state = MeshState::Disabled;
    }

    /// Returns the current session ID, if active.
    pub fn current_session_id(&self) -> Option<[u8; 16]> {
        self.current_advertisement
            .as_ref()
            .map(|ad| *ad.session_id())
    }

    /// Checks if a peer's public key has already completed exchange.
    pub fn is_peer_completed(&self, public_key: &[u8; 32]) -> bool {
        self.completed_peer_keys.contains(public_key)
    }

    /// Marks a peer's public key as having completed exchange.
    pub fn mark_peer_completed(&mut self, public_key: [u8; 32]) {
        self.completed_peer_keys.insert(public_key);
    }

    /// Returns the list of currently discovered peers.
    pub fn discovered_peers(&self) -> &[DiscoveredPeer] {
        &self.discovered_peers
    }

    /// Adds a discovered peer, respecting the max concurrent limit.
    ///
    /// If at capacity, the weakest signal (highest RSSI magnitude) peer
    /// is evicted to make room.
    pub fn add_discovered_peer(&mut self, peer: DiscoveredPeer) {
        // Check if peer already known (update RSSI)
        if let Some(existing) = self
            .discovered_peers
            .iter_mut()
            .find(|p| p.device_id == peer.device_id)
        {
            existing.update_rssi(peer.rssi);
            return;
        }

        if self.discovered_peers.len() >= self.config.max_concurrent_discoveries {
            // Evict weakest signal (most negative RSSI = weakest)
            if let Some(weakest_idx) = self
                .discovered_peers
                .iter()
                .enumerate()
                .min_by_key(|(_, p)| p.rssi)
                .map(|(i, _)| i)
            {
                // Only evict if new peer has stronger signal
                if peer.rssi > self.discovered_peers[weakest_idx].rssi {
                    self.discovered_peers.remove(weakest_idx);
                } else {
                    return; // New peer is weaker, don't add
                }
            }
        }

        self.discovered_peers.push(peer);
        self.state = MeshState::Active {
            discovered_count: self.discovered_peers.len(),
        };
    }
}
