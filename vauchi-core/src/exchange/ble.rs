// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE Exchange Module
//!
//! Bluetooth Low Energy proximity verification and contact exchange for mobile platforms.
//! This module provides trait definitions, session management, and mock implementations
//! for BLE-based contact exchange.

use super::{ProximityError, ProximityVerifier};
use crate::crypto::{PublicKey, SigningKeyPair};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// A discovered BLE device.
#[derive(Debug, Clone)]
pub struct BLEDevice {
    /// Unique device identifier
    pub id: String,
    /// Device name (if advertised)
    pub name: Option<String>,
    /// Signal strength in dBm
    pub rssi: i16,
    /// Vauchi exchange token (from advertisement)
    pub exchange_token: Option<[u8; 32]>,
}

impl BLEDevice {
    /// Creates a new BLE device.
    pub fn new(id: &str, rssi: i16) -> Self {
        BLEDevice {
            id: id.to_string(),
            name: None,
            rssi,
            exchange_token: None,
        }
    }

    /// Creates a device with a name.
    pub fn with_name(id: &str, name: &str, rssi: i16) -> Self {
        BLEDevice {
            id: id.to_string(),
            name: Some(name.to_string()),
            rssi,
            exchange_token: None,
        }
    }

    /// Sets the exchange token.
    pub fn with_exchange_token(mut self, token: [u8; 32]) -> Self {
        self.exchange_token = Some(token);
        self
    }
}

/// BLE proximity verification trait.
///
/// Extends the base `ProximityVerifier` with BLE-specific capabilities
/// for discovering and measuring distance to nearby devices.
pub trait BLEProximityVerifier: ProximityVerifier {
    /// Discovers nearby BLE devices advertising Vauchi exchange.
    fn discover_nearby(&self, timeout: Duration) -> Result<Vec<BLEDevice>, ProximityError>;

    /// Estimates distance to a device based on RSSI.
    ///
    /// Returns estimated distance in meters.
    fn estimate_distance(&self, device: &BLEDevice) -> Result<f32, ProximityError>;

    /// Checks if a device is within the specified range.
    fn is_within_range(&self, device: &BLEDevice, max_meters: f32) -> bool {
        self.estimate_distance(device)
            .map(|d| d <= max_meters)
            .unwrap_or(false)
    }

    /// Verifies proximity to a specific device.
    ///
    /// Returns Ok if the device is within acceptable range (default 2 meters).
    fn verify_device_proximity(&self, device: &BLEDevice) -> Result<(), ProximityError> {
        const DEFAULT_MAX_DISTANCE: f32 = 2.0;

        if self.is_within_range(device, DEFAULT_MAX_DISTANCE) {
            Ok(())
        } else {
            Err(ProximityError::TooFar)
        }
    }
}

/// Mock BLE verifier for testing.
pub struct MockBLEVerifier {
    /// Pre-configured nearby devices.
    pub devices: Vec<BLEDevice>,
    /// Simulated distance for all devices.
    pub simulated_distance: f32,
    /// Whether operations should succeed.
    pub should_succeed: bool,
}

impl MockBLEVerifier {
    /// Creates a mock verifier with nearby devices at the given distance.
    pub fn new(devices: Vec<BLEDevice>, distance: f32) -> Self {
        MockBLEVerifier {
            devices,
            simulated_distance: distance,
            should_succeed: true,
        }
    }

    /// Creates a mock verifier that always succeeds with default devices.
    pub fn success_at_distance(distance: f32) -> Self {
        let device = BLEDevice::new("mock-device-1", -50);
        MockBLEVerifier {
            devices: vec![device],
            simulated_distance: distance,
            should_succeed: true,
        }
    }

    /// Creates a mock verifier that always fails.
    pub fn failure() -> Self {
        MockBLEVerifier {
            devices: vec![],
            simulated_distance: 100.0,
            should_succeed: false,
        }
    }
}

impl ProximityVerifier for MockBLEVerifier {
    fn emit_challenge(&self, _challenge: &[u8; 16]) -> Result<(), ProximityError> {
        if self.should_succeed {
            Ok(())
        } else {
            Err(ProximityError::DeviceError("Mock failure".into()))
        }
    }

    fn listen_for_response(&self, _timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        if self.should_succeed {
            Ok(vec![0u8; 16])
        } else {
            Err(ProximityError::Timeout)
        }
    }

    fn verify_response(&self, _challenge: &[u8; 16], _response: &[u8]) -> bool {
        self.should_succeed
    }
}

impl BLEProximityVerifier for MockBLEVerifier {
    fn discover_nearby(&self, _timeout: Duration) -> Result<Vec<BLEDevice>, ProximityError> {
        if self.should_succeed {
            Ok(self.devices.clone())
        } else {
            Err(ProximityError::DeviceError("BLE discovery failed".into()))
        }
    }

    fn estimate_distance(&self, _device: &BLEDevice) -> Result<f32, ProximityError> {
        if self.should_succeed {
            Ok(self.simulated_distance)
        } else {
            Err(ProximityError::DeviceError(
                "Distance estimation failed".into(),
            ))
        }
    }
}

// ============================================================
// BLE Advertisement
// ============================================================

/// Vauchi BLE service UUID (custom 128-bit UUID)
pub const VAUCHI_BLE_SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";

/// BLE advertisement for Vauchi exchange.
///
/// Contains the exchange token and signature for discovery by other devices.
#[derive(Debug, Clone)]
pub struct BLEAdvertisement {
    /// Exchange token (32 bytes)
    exchange_token: [u8; 32],
    /// Public key of the advertiser
    public_key: [u8; 32],
    /// Signature over exchange token
    signature: [u8; 64],
}

impl BLEAdvertisement {
    /// Create a new BLE advertisement.
    pub fn new(keypair: &SigningKeyPair, exchange_token: [u8; 32]) -> Self {
        let signature = keypair.sign(&exchange_token);
        BLEAdvertisement {
            exchange_token,
            public_key: *keypair.public_key().as_bytes(),
            signature: *signature.as_bytes(),
        }
    }

    /// Get the exchange token.
    pub fn exchange_token(&self) -> &[u8; 32] {
        &self.exchange_token
    }

    /// Get the service UUID.
    pub fn service_uuid(&self) -> &str {
        VAUCHI_BLE_SERVICE_UUID
    }

    /// Verify the signature against a public key.
    pub fn verify_signature(&self, public_key: &PublicKey) -> bool {
        use crate::crypto::Signature;
        let sig = Signature::from_bytes(self.signature);
        public_key.verify(&self.exchange_token, &sig)
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(128);
        bytes.extend_from_slice(&self.exchange_token);
        bytes.extend_from_slice(&self.public_key);
        bytes.extend_from_slice(&self.signature);
        bytes
    }

    /// Parse from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BLEError> {
        if bytes.len() < 128 {
            return Err(BLEError::InvalidPayload("Too short".into()));
        }

        let mut exchange_token = [0u8; 32];
        let mut public_key = [0u8; 32];
        let mut signature = [0u8; 64];

        exchange_token.copy_from_slice(&bytes[0..32]);
        public_key.copy_from_slice(&bytes[32..64]);
        signature.copy_from_slice(&bytes[64..128]);

        Ok(BLEAdvertisement {
            exchange_token,
            public_key,
            signature,
        })
    }
}

// ============================================================
// BLE Exchange State
// ============================================================

/// State of a BLE exchange session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BLEExchangeState {
    /// Session created but not active.
    Idle,
    /// Advertising our presence.
    Advertising,
    /// Scanning for peers.
    Scanning,
    /// Connected to a peer.
    Connected {
        /// Peer's exchange token
        #[serde(with = "crate::exchange::nfc::hex_array_32")]
        peer_token: [u8; 32],
        /// Peer's device ID
        peer_device_id: String,
    },
    /// Exchange completed successfully.
    Completed,
    /// Session timed out.
    TimedOut,
    /// Session was cancelled.
    Cancelled,
    /// An error occurred.
    Error(String),
}

// ============================================================
// BLE Exchange Error
// ============================================================

/// BLE exchange errors.
#[derive(Debug, Clone)]
pub enum BLEError {
    /// Invalid payload format.
    InvalidPayload(String),
    /// Not connected to a peer.
    NotConnected,
    /// Device doesn't have exchange token.
    NoExchangeToken,
    /// Session already in progress.
    SessionInProgress,
    /// Session timed out.
    Timeout,
}

impl std::fmt::Display for BLEError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BLEError::InvalidPayload(msg) => write!(f, "Invalid BLE payload: {}", msg),
            BLEError::NotConnected => write!(f, "Not connected to peer"),
            BLEError::NoExchangeToken => write!(f, "Device has no exchange token"),
            BLEError::SessionInProgress => write!(f, "Session already in progress"),
            BLEError::Timeout => write!(f, "Session timed out"),
        }
    }
}

impl std::error::Error for BLEError {}

// ============================================================
// BLE Exchange Session
// ============================================================

/// Manages a BLE exchange session.
///
/// Handles advertising, scanning, connection, and data exchange with a peer.
pub struct BLEExchangeSession {
    /// Our exchange token
    exchange_token: [u8; 32],
    /// Current state
    state: BLEExchangeState,
    /// Session timeout
    timeout: Duration,
    /// Session start time (for timeout tracking)
    started_at: Option<Instant>,
    /// Our contact data to exchange
    local_contact_data: Option<Vec<u8>>,
    /// Peer's contact data (received during exchange)
    peer_contact_data: Option<Vec<u8>>,
}

impl BLEExchangeSession {
    /// Create a new exchange session.
    pub fn new(_keypair: &SigningKeyPair) -> Self {
        use ring::rand::{SecureRandom, SystemRandom};
        let rng = SystemRandom::new();
        let mut exchange_token = [0u8; 32];
        rng.fill(&mut exchange_token).expect("RNG failed");

        BLEExchangeSession {
            exchange_token,
            state: BLEExchangeState::Idle,
            timeout: Duration::from_secs(60),
            started_at: None,
            local_contact_data: None,
            peer_contact_data: None,
        }
    }

    /// Create a session with custom timeout.
    pub fn with_timeout(keypair: &SigningKeyPair, timeout: Duration) -> Self {
        let mut session = Self::new(keypair);
        session.timeout = timeout;
        session
    }

    /// Get the current state.
    pub fn state(&self) -> &BLEExchangeState {
        &self.state
    }

    /// Get our exchange token.
    pub fn exchange_token(&self) -> Option<&[u8; 32]> {
        Some(&self.exchange_token)
    }

    /// Start advertising our presence.
    pub fn start_advertising(&mut self) -> Result<(), BLEError> {
        match &self.state {
            BLEExchangeState::Idle => {
                self.state = BLEExchangeState::Advertising;
                self.started_at = Some(Instant::now());
                Ok(())
            }
            _ => Err(BLEError::SessionInProgress),
        }
    }

    /// Start scanning for peers.
    pub fn start_scanning(&mut self) -> Result<(), BLEError> {
        match &self.state {
            BLEExchangeState::Idle => {
                self.state = BLEExchangeState::Scanning;
                self.started_at = Some(Instant::now());
                Ok(())
            }
            _ => Err(BLEError::SessionInProgress),
        }
    }

    /// Connect to a discovered device.
    pub fn connect_to_device(&mut self, device: &BLEDevice) -> Result<(), BLEError> {
        // Require exchange token
        let peer_token = device.exchange_token.ok_or(BLEError::NoExchangeToken)?;

        match &self.state {
            BLEExchangeState::Scanning => {
                self.state = BLEExchangeState::Connected {
                    peer_token,
                    peer_device_id: device.id.clone(),
                };
                Ok(())
            }
            _ => Err(BLEError::NotConnected),
        }
    }

    /// Set our contact data to exchange.
    pub fn set_contact_data(&mut self, data: &[u8]) {
        self.local_contact_data = Some(data.to_vec());
    }

    /// Get our local contact data.
    pub fn get_local_contact_data(&self) -> Option<&[u8]> {
        self.local_contact_data.as_deref()
    }

    /// Get peer's contact data (after exchange).
    pub fn get_peer_contact_data(&self) -> Option<&[u8]> {
        self.peer_contact_data.as_deref()
    }

    /// Check for timeout and update state.
    pub fn check_timeout(&mut self) {
        if let Some(started) = self.started_at {
            if Instant::now().duration_since(started) >= self.timeout {
                self.state = BLEExchangeState::TimedOut;
            }
        }
    }

    /// Cancel the session.
    pub fn cancel(&mut self) {
        self.state = BLEExchangeState::Cancelled;
    }
}
