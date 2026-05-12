// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE Exchange Module
//!
//! Bluetooth Low Energy proximity verification and contact exchange for mobile platforms.
//! This module provides trait definitions, session management, and mock implementations
//! for BLE-based contact exchange.

use super::exchange_payload::{
    EXCHANGE_PAYLOAD_SIZE, ParsedPayload, build_exchange_payload, is_payload_expired,
    parse_exchange_payload, verify_payload_signature,
};
use super::x3dh::X3DHKeyPair;
use super::{ExchangeError, ProximityError, ProximityVerifier};
use crate::crypto::{PublicKey, SigningKeyPair};
use crate::identity::Identity;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

mod hex_array_32 {
    use serde::{Deserialize, Deserializer, Serializer};
    /// Serializes a 32-byte array to a hex-encoded string for BLE payload transmission.
    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }
    /// Deserializes a 32-byte array from a hex-encoded string.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid length"))
    }
}

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
    /// BLE RSSI estimates distance but does not cryptographically prove proximity.
    /// RSSI can be amplified, relayed, or is inaccurate through walls — Medium confidence.
    fn confidence_level(&self) -> super::ProximityConfidence {
        super::ProximityConfidence::Medium
    }

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
#[non_exhaustive]
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
        #[serde(with = "hex_array_32")]
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
#[non_exhaustive]
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
        use crate::crypto::random_bytes;
        let exchange_token: [u8; 32] = random_bytes();

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
        self.check_timeout_at(Instant::now());
    }

    /// Check for timeout at a given point in time.
    pub fn check_timeout_at(&mut self, now: Instant) {
        if let Some(started) = self.started_at
            && now.duration_since(started) >= self.timeout
        {
            self.state = BLEExchangeState::TimedOut;
        }
    }

    /// Cancel the session.
    pub fn cancel(&mut self) {
        self.state = BLEExchangeState::Cancelled;
    }
}

// ============================================================
// BLE Exchange Payload (174 bytes)
// ============================================================

/// BLE payload magic bytes.
const BLE_MAGIC: &[u8; 4] = b"VBLE";

/// BLE payload expiry in seconds (60 seconds).
const BLE_EXPIRY_SECONDS: u64 = 60;

/// BLE exchange payload size.
pub const BLE_PAYLOAD_SIZE: usize = EXCHANGE_PAYLOAD_SIZE;

/// GATT characteristic UUID for exchange payload (Read+Notify).
pub const CHAR_EXCHANGE_PAYLOAD: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";

/// GATT characteristic UUID for card exchange (Write+Notify).
pub const CHAR_CARD_EXCHANGE: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";

/// GATT characteristic UUID for challenge-response (Write+Notify).
pub const CHAR_CHALLENGE: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567893";

/// BLE handshake write characteristic (Write with Response).
/// Used for KeyOffer and committed payloads from initiator to responder.
pub const CHAR_HANDSHAKE_WRITE: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567894";

/// BLE handshake notify characteristic (Notify).
/// Used for KeyAck, committed payloads, and reveal from responder to initiator.
pub const CHAR_HANDSHAKE_NOTIFY: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567895";

/// BLE data write characteristic (Write without Response).
/// Used for chunked encrypted card data from initiator to responder.
pub const CHAR_DATA_WRITE: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567896";

/// BLE data notify characteristic (Notify).
/// Used for chunked encrypted card data from responder to initiator.
pub const CHAR_DATA_NOTIFY: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567897";

/// BLE diagnostic service UUID (separate from exchange service).
pub const VAUCHI_BLE_DIAGNOSTIC_SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef12345678a0";

/// Minimum MTU required for BLE exchange (handshake packet = 113 bytes + ATT overhead).
pub const BLE_MIN_MTU: usize = 120;

/// Default conservative MTU usable bytes (185 MTU - 3 ATT header - 4 chunk overhead).
pub const BLE_DEFAULT_USABLE: usize = 178;

/// BLE exchange payload.
///
/// 174-byte payload exchanged during BLE GATT connection:
/// - Magic "VBLE" (4 bytes)
/// - Version (1 byte)
/// - Flags (1 byte)
/// - Identity key — Ed25519 signing public key (32 bytes)
/// - Exchange key — fresh ephemeral X25519 public key (32 bytes)
/// - Token — random session token (32 bytes)
/// - Timestamp — Unix timestamp (8 bytes)
/// - Signature — Ed25519 signature over all preceding fields (64 bytes)
#[derive(Clone, Debug)]
pub struct ExchangeBle {
    inner: ParsedPayload,
}

impl ExchangeBle {
    /// Generates a new BLE exchange payload.
    pub fn generate(identity: &Identity, ephemeral: &X3DHKeyPair) -> Self {
        use crate::crypto::random_bytes;

        let token: [u8; 32] = random_bytes();

        let timestamp = super::now_secs();

        Self::generate_with_timestamp(identity, ephemeral, token, timestamp)
    }

    /// Generates with explicit timestamp (for testing).
    pub fn generate_with_timestamp(
        identity: &Identity,
        ephemeral: &X3DHKeyPair,
        token: [u8; 32],
        timestamp: u64,
    ) -> Self {
        let bytes = build_exchange_payload(BLE_MAGIC, identity, ephemeral, token, timestamp);
        let inner = parse_exchange_payload(&bytes, BLE_MAGIC, ExchangeError::InvalidBleFormat)
            .expect("Freshly built payload should parse");
        ExchangeBle { inner }
    }

    /// Returns the identity (Ed25519 signing) key.
    pub fn identity_key(&self) -> &[u8; 32] {
        &self.inner.identity_key
    }

    /// Returns the exchange (X25519 ephemeral) key.
    pub fn exchange_key(&self) -> &[u8; 32] {
        &self.inner.exchange_key
    }

    /// Returns the session token.
    pub fn token(&self) -> &[u8; 32] {
        &self.inner.token
    }

    /// Returns the timestamp.
    pub fn timestamp(&self) -> u64 {
        self.inner.timestamp
    }

    /// Checks if the payload has expired.
    pub fn is_expired(&self) -> bool {
        is_payload_expired(self.inner.timestamp, BLE_EXPIRY_SECONDS)
    }

    /// Verifies the Ed25519 signature.
    pub fn verify_signature(&self) -> bool {
        verify_payload_signature(BLE_MAGIC, &self.inner)
    }

    /// Serializes the payload to bytes.
    pub fn to_bytes(&self) -> [u8; BLE_PAYLOAD_SIZE] {
        let mut buf = [0u8; BLE_PAYLOAD_SIZE];
        buf[0..4].copy_from_slice(BLE_MAGIC);
        buf[4] = self.inner.version;
        buf[5] = self.inner.flags;
        buf[6..38].copy_from_slice(&self.inner.identity_key);
        buf[38..70].copy_from_slice(&self.inner.exchange_key);
        buf[70..102].copy_from_slice(&self.inner.token);
        buf[102..110].copy_from_slice(&self.inner.timestamp.to_be_bytes());
        buf[110..174].copy_from_slice(&self.inner.signature);
        buf
    }

    /// Parses the payload from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ExchangeError> {
        let inner = parse_exchange_payload(bytes, BLE_MAGIC, ExchangeError::InvalidBleFormat)?;
        Ok(ExchangeBle { inner })
    }
}

// ============================================================
// BLE Transport Trait (platform abstraction)
// ============================================================

/// Trait for platform-specific BLE transport operations.
///
/// Platform implementations (Android, iOS) implement this trait;
/// tests use `MockBLETransport`.
///
/// **Deprecated (ADR-031):** Use `Command::Ble*` / `Event::Ble*`
/// instead. Core no longer calls hardware directly — frontends execute BLE commands.
#[deprecated(note = "ADR-031: use Command/Event for BLE")]
pub trait BLETransport: Send + Sync {
    /// Start advertising our exchange payload.
    fn start_advertising(&self, payload: &ExchangeBle) -> Result<(), BLEError>;

    /// Start scanning for nearby exchange advertisers.
    fn start_scanning(&self) -> Result<(), BLEError>;

    /// Stop advertising and/or scanning.
    fn stop(&self);

    /// Connect to a discovered device by ID.
    fn connect(&self, device_id: &str) -> Result<(), BLEError>;

    /// Write data to a GATT characteristic.
    fn write_characteristic(&self, uuid: &str, data: &[u8]) -> Result<(), BLEError>;

    /// Read data from a GATT characteristic.
    fn read_characteristic(&self, uuid: &str) -> Result<Vec<u8>, BLEError>;

    /// Disconnect from the current device.
    fn disconnect(&self) -> Result<(), BLEError>;
}

/// Mock BLE transport for testing.
///
/// **Deprecated (ADR-031):** Use `Event::Ble*` injection for testing.
#[deprecated(note = "ADR-031: use Event injection for BLE testing")]
pub struct MockBLETransport {
    /// Payload to return when reading CHAR_EXCHANGE_PAYLOAD.
    pub peer_payload: std::sync::Mutex<Option<Vec<u8>>>,
    /// Whether operations should succeed.
    pub should_succeed: bool,
    /// Written characteristic data (uuid, data) for assertions.
    pub written: std::sync::Mutex<Vec<(String, Vec<u8>)>>,
}

impl MockBLETransport {
    /// Creates a mock that succeeds and returns the given peer payload.
    pub fn with_peer_payload(payload: &[u8]) -> Self {
        MockBLETransport {
            peer_payload: std::sync::Mutex::new(Some(payload.to_vec())),
            should_succeed: true,
            written: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Creates a mock that always fails.
    pub fn failing() -> Self {
        MockBLETransport {
            peer_payload: std::sync::Mutex::new(None),
            should_succeed: false,
            written: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Returns all written (uuid, data) pairs.
    pub fn get_written(&self) -> Vec<(String, Vec<u8>)> {
        self.written.lock().expect("mutex poisoned").clone()
    }
}

#[allow(deprecated)]
impl BLETransport for MockBLETransport {
    fn start_advertising(&self, _payload: &ExchangeBle) -> Result<(), BLEError> {
        if self.should_succeed {
            Ok(())
        } else {
            Err(BLEError::InvalidPayload("Mock failure".into()))
        }
    }

    fn start_scanning(&self) -> Result<(), BLEError> {
        if self.should_succeed {
            Ok(())
        } else {
            Err(BLEError::Timeout)
        }
    }

    fn stop(&self) {}

    fn connect(&self, _device_id: &str) -> Result<(), BLEError> {
        if self.should_succeed {
            Ok(())
        } else {
            Err(BLEError::NotConnected)
        }
    }

    fn write_characteristic(&self, uuid: &str, data: &[u8]) -> Result<(), BLEError> {
        if self.should_succeed {
            self.written
                .lock()
                .expect("mutex poisoned")
                .push((uuid.to_string(), data.to_vec()));
            Ok(())
        } else {
            Err(BLEError::NotConnected)
        }
    }

    fn read_characteristic(&self, uuid: &str) -> Result<Vec<u8>, BLEError> {
        if !self.should_succeed {
            return Err(BLEError::NotConnected);
        }

        if uuid == CHAR_EXCHANGE_PAYLOAD
            && let Some(payload) = self.peer_payload.lock().expect("mutex poisoned").as_ref()
        {
            return Ok(payload.clone());
        }

        Err(BLEError::InvalidPayload(
            "No data for characteristic".into(),
        ))
    }

    fn disconnect(&self) -> Result<(), BLEError> {
        Ok(())
    }
}
