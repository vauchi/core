//! Device Management Module
//!
//! Handles multi-device support for Vauchi identities.
//! Each device gets unique communication keys derived from the master seed.

use crate::crypto::{Signature, SigningKeyPair, HKDF};
use crate::exchange::X3DHKeyPair;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Domain separation constants for device key derivation.
const DEVICE_ID_INFO: &[u8] = b"Vauchi_Device_ID";
const DEVICE_EXCHANGE_INFO: &[u8] = b"Vauchi_Device_Exchange";

/// Maximum number of linked devices per identity.
pub const MAX_DEVICES: usize = 10;

/// Device-related errors.
#[derive(Error, Debug)]
pub enum DeviceError {
    #[error("Maximum devices ({MAX_DEVICES}) reached")]
    MaxDevicesReached,
    #[error("Device not found")]
    DeviceNotFound,
    #[error("Cannot remove last device")]
    CannotRemoveLastDevice,
    #[error("Device already exists")]
    DeviceAlreadyExists,
    #[error("Invalid device registry signature")]
    InvalidRegistrySignature,
    #[error("Device name cannot be empty")]
    EmptyDeviceName,
}

/// Device-specific cryptographic material and metadata.
pub struct DeviceInfo {
    /// Unique device identifier (32 bytes, derived from master_seed + device_index).
    device_id: [u8; 32],
    /// Device index used for deterministic key derivation.
    device_index: u32,
    /// Device-specific X25519 keypair for communication.
    device_exchange_keypair: X3DHKeyPair,
    /// Human-readable device name.
    device_name: String,
    /// Unix timestamp when this device was created.
    created_at: u64,
}

impl DeviceInfo {
    /// Derives device keys from master seed and device index.
    pub fn derive(master_seed: &[u8; 32], device_index: u32, device_name: String) -> Self {
        let index_bytes = device_index.to_le_bytes();

        // Derive device ID: HKDF(master_seed, index, "Vauchi_Device_ID")
        let device_id = HKDF::derive_key(Some(master_seed), &index_bytes, DEVICE_ID_INFO);

        // Derive device exchange key seed
        let exchange_seed = HKDF::derive_key(Some(master_seed), &index_bytes, DEVICE_EXCHANGE_INFO);
        let device_exchange_keypair = X3DHKeyPair::from_bytes(exchange_seed);

        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            device_id,
            device_index,
            device_exchange_keypair,
            device_name,
            created_at,
        }
    }

    /// Returns the device ID.
    pub fn device_id(&self) -> &[u8; 32] {
        &self.device_id
    }

    /// Returns the device index.
    pub fn device_index(&self) -> u32 {
        self.device_index
    }

    /// Returns the device exchange public key.
    pub fn exchange_public_key(&self) -> &[u8; 32] {
        self.device_exchange_keypair.public_key()
    }

    /// Returns the device exchange keypair.
    pub fn exchange_keypair(&self) -> &X3DHKeyPair {
        &self.device_exchange_keypair
    }

    /// Returns the device name.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Sets the device name.
    pub fn set_device_name(&mut self, name: String) -> Result<(), DeviceError> {
        if name.is_empty() {
            return Err(DeviceError::EmptyDeviceName);
        }
        self.device_name = name;
        Ok(())
    }

    /// Returns the creation timestamp.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Converts to a RegisteredDevice for the registry.
    ///
    /// Note: The master_seed parameter is intentionally unused here but included
    /// for API consistency with device linking flows where the seed is available.
    pub fn to_registered(&self, _master_seed: &[u8; 32]) -> RegisteredDevice {
        RegisteredDevice {
            device_id: self.device_id,
            exchange_public_key: *self.device_exchange_keypair.public_key(),
            device_name: self.device_name.clone(),
            created_at: self.created_at,
            revoked: false,
            revoked_at: None,
        }
    }
}

/// A device entry in the registry (public information only).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegisteredDevice {
    /// Unique device ID.
    pub device_id: [u8; 32],
    /// Device's X25519 public key for receiving messages.
    pub exchange_public_key: [u8; 32],
    /// Human-readable name.
    pub device_name: String,
    /// Creation timestamp.
    pub created_at: u64,
    /// Whether this device has been revoked.
    pub revoked: bool,
    /// Revocation timestamp (if revoked).
    pub revoked_at: Option<u64>,
}

impl RegisteredDevice {
    /// Returns the device ID as hex string.
    pub fn device_id_hex(&self) -> String {
        hex::encode(self.device_id)
    }

    /// Returns whether this device is active (not revoked).
    pub fn is_active(&self) -> bool {
        !self.revoked
    }
}

/// Registry of all devices linked to an identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRegistry {
    /// All registered devices.
    devices: Vec<RegisteredDevice>,
    /// Version counter (increments on each change).
    version: u64,
    /// Signature over the registry by the identity signing key.
    #[serde(with = "signature_serde")]
    signature: [u8; 64],
}

mod signature_serde {
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(sig: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(sig))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid signature length"))
    }
}

impl DeviceRegistry {
    /// Creates a new registry with a single device.
    pub fn new(device: RegisteredDevice, signing_key: &SigningKeyPair) -> Self {
        let mut registry = Self {
            devices: vec![device],
            version: 1,
            signature: [0u8; 64],
        };
        registry.sign(signing_key);
        registry
    }

    /// Returns the registry version.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Returns all devices (including revoked).
    pub fn all_devices(&self) -> &[RegisteredDevice] {
        &self.devices
    }

    /// Returns only active (non-revoked) devices.
    pub fn active_devices(&self) -> Vec<&RegisteredDevice> {
        self.devices.iter().filter(|d| d.is_active()).collect()
    }

    /// Returns the number of active devices.
    pub fn active_count(&self) -> usize {
        self.devices.iter().filter(|d| d.is_active()).count()
    }

    /// Finds a device by ID.
    pub fn find_device(&self, device_id: &[u8; 32]) -> Option<&RegisteredDevice> {
        self.devices.iter().find(|d| &d.device_id == device_id)
    }

    /// Adds a new device to the registry.
    pub fn add_device(
        &mut self,
        device: RegisteredDevice,
        signing_key: &SigningKeyPair,
    ) -> Result<(), DeviceError> {
        if self.active_count() >= MAX_DEVICES {
            return Err(DeviceError::MaxDevicesReached);
        }

        if self.find_device(&device.device_id).is_some() {
            return Err(DeviceError::DeviceAlreadyExists);
        }

        self.devices.push(device);
        self.version += 1;
        self.sign(signing_key);
        Ok(())
    }

    /// Revokes a device by ID.
    pub fn revoke_device(
        &mut self,
        device_id: &[u8; 32],
        signing_key: &SigningKeyPair,
    ) -> Result<(), DeviceError> {
        if self.active_count() <= 1 {
            // Check if we're trying to revoke the last active device
            if let Some(device) = self.find_device(device_id) {
                if device.is_active() {
                    return Err(DeviceError::CannotRemoveLastDevice);
                }
            }
        }

        let device = self
            .devices
            .iter_mut()
            .find(|d| &d.device_id == device_id)
            .ok_or(DeviceError::DeviceNotFound)?;

        if device.revoked {
            return Ok(()); // Already revoked
        }

        device.revoked = true;
        device.revoked_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );

        self.version += 1;
        self.sign(signing_key);
        Ok(())
    }

    /// Signs the registry with the identity signing key.
    fn sign(&mut self, signing_key: &SigningKeyPair) {
        let data = self.signing_data();
        let signature = signing_key.sign(&data);
        self.signature = *signature.as_bytes();
    }

    /// Verifies the registry signature.
    pub fn verify(&self, public_key: &crate::crypto::PublicKey) -> bool {
        let data = self.signing_data();
        let signature = Signature::from_bytes(self.signature);
        public_key.verify(&data, &signature)
    }

    /// Returns the data to be signed.
    fn signing_data(&self) -> Vec<u8> {
        // Sign: version || device_count || [device_id || exchange_public_key || revoked]*
        let mut data = Vec::new();
        data.extend_from_slice(&self.version.to_le_bytes());
        data.extend_from_slice(&(self.devices.len() as u32).to_le_bytes());
        for device in &self.devices {
            data.extend_from_slice(&device.device_id);
            data.extend_from_slice(&device.exchange_public_key);
            data.push(if device.revoked { 1 } else { 0 });
        }
        data
    }

    /// Returns the next available device index.
    pub fn next_device_index(&self) -> u32 {
        self.devices.iter().map(|_| 1u32).sum::<u32>()
    }

    /// Returns the total number of devices (including revoked).
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// Adds a device without re-signing (for internal use during linking).
    ///
    /// This is used when the registry signature will be updated separately.
    pub fn add_device_unsigned(&mut self, device: RegisteredDevice) -> Result<(), DeviceError> {
        if self.active_count() >= MAX_DEVICES {
            return Err(DeviceError::MaxDevicesReached);
        }

        if self.find_device(&device.device_id).is_some() {
            return Err(DeviceError::DeviceAlreadyExists);
        }

        self.devices.push(device);
        self.version += 1;
        Ok(())
    }

    /// Serializes the registry to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Registry serialization should not fail")
    }

    /// Deserializes a registry from JSON.
    pub fn from_json(json: &str) -> Result<Self, DeviceError> {
        serde_json::from_str(json).map_err(|_| DeviceError::InvalidRegistrySignature)
    }

    /// Applies a revocation certificate to the registry.
    ///
    /// This is used when receiving a revocation certificate from a contact
    /// to update our local knowledge of their device registry.
    pub fn apply_revocation(
        &mut self,
        certificate: &DeviceRevocationCertificate,
        public_key: &crate::crypto::PublicKey,
    ) -> Result<(), DeviceError> {
        // Verify certificate signature
        if !certificate.verify(public_key) {
            return Err(DeviceError::InvalidRegistrySignature);
        }

        // Find and revoke the device
        let device = self
            .devices
            .iter_mut()
            .find(|d| &d.device_id == certificate.device_id())
            .ok_or(DeviceError::DeviceNotFound)?;

        device.revoked = true;
        device.revoked_at = Some(certificate.revoked_at());
        self.version += 1;

        Ok(())
    }
}

// ============================================================
// Phase 5: Device Revocation Types
// ============================================================

/// A signed certificate proving that a device has been revoked.
///
/// This certificate can be shared with contacts so they stop sending
/// messages to the revoked device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRevocationCertificate {
    /// ID of the revoked device.
    device_id: [u8; 32],
    /// Reason for revocation (optional).
    reason: String,
    /// Timestamp when revoked.
    revoked_at: u64,
    /// Signature over the certificate by the identity signing key.
    #[serde(with = "signature_serde")]
    signature: [u8; 64],
}

impl DeviceRevocationCertificate {
    /// Creates a new revocation certificate.
    pub fn create(device_id: &[u8; 32], reason: String, signing_key: &SigningKeyPair) -> Self {
        let revoked_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut certificate = Self {
            device_id: *device_id,
            reason,
            revoked_at,
            signature: [0u8; 64],
        };

        certificate.sign(signing_key);
        certificate
    }

    /// Returns the revoked device ID.
    pub fn device_id(&self) -> &[u8; 32] {
        &self.device_id
    }

    /// Returns the revocation timestamp.
    pub fn revoked_at(&self) -> u64 {
        self.revoked_at
    }

    /// Returns the revocation reason.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Verifies the certificate signature.
    pub fn verify(&self, public_key: &crate::crypto::PublicKey) -> bool {
        let data = self.signing_data();
        let signature = Signature::from_bytes(self.signature);
        public_key.verify(&data, &signature)
    }

    /// Signs the certificate.
    fn sign(&mut self, signing_key: &SigningKeyPair) {
        let data = self.signing_data();
        let signature = signing_key.sign(&data);
        self.signature = *signature.as_bytes();
    }

    /// Returns the data to be signed.
    fn signing_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"REVOKE:");
        data.extend_from_slice(&self.device_id);
        data.extend_from_slice(&self.revoked_at.to_le_bytes());
        data.extend_from_slice(self.reason.as_bytes());
        data
    }

    /// Serializes to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Certificate serialization should not fail")
    }

    /// Deserializes from JSON.
    pub fn from_json(json: &str) -> Result<Self, DeviceError> {
        serde_json::from_str(json).map_err(|_| DeviceError::InvalidRegistrySignature)
    }
}

/// A message broadcasting the current device registry to contacts.
///
/// Contacts use this to know which devices to send updates to.
/// Only active (non-revoked) devices are included.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryBroadcast {
    /// Version of the registry.
    version: u64,
    /// Active devices (ID -> exchange public key).
    active_devices: Vec<BroadcastDevice>,
    /// Timestamp of broadcast.
    timestamp: u64,
    /// Signature over the broadcast.
    #[serde(with = "signature_serde")]
    signature: [u8; 64],
}

/// A device entry in the broadcast (minimal info for contacts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastDevice {
    /// Device ID.
    pub device_id: [u8; 32],
    /// Device exchange public key for sending messages.
    pub exchange_public_key: [u8; 32],
}

impl RegistryBroadcast {
    /// Creates a new broadcast from a registry.
    pub fn new(registry: &DeviceRegistry, signing_key: &SigningKeyPair) -> Self {
        let active_devices: Vec<BroadcastDevice> = registry
            .active_devices()
            .iter()
            .map(|d| BroadcastDevice {
                device_id: d.device_id,
                exchange_public_key: d.exchange_public_key,
            })
            .collect();

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut broadcast = Self {
            version: registry.version(),
            active_devices,
            timestamp,
            signature: [0u8; 64],
        };

        broadcast.sign(signing_key);
        broadcast
    }

    /// Returns the registry version.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Returns the number of active devices.
    pub fn active_device_count(&self) -> usize {
        self.active_devices.len()
    }

    /// Checks if a device is in the broadcast.
    pub fn contains_device(&self, device_id: &[u8; 32]) -> bool {
        self.active_devices
            .iter()
            .any(|d| &d.device_id == device_id)
    }

    /// Returns the active devices.
    pub fn active_devices(&self) -> &[BroadcastDevice] {
        &self.active_devices
    }

    /// Verifies the broadcast signature.
    pub fn verify(&self, public_key: &crate::crypto::PublicKey) -> bool {
        let data = self.signing_data();
        let signature = Signature::from_bytes(self.signature);
        public_key.verify(&data, &signature)
    }

    /// Signs the broadcast.
    fn sign(&mut self, signing_key: &SigningKeyPair) {
        let data = self.signing_data();
        let signature = signing_key.sign(&data);
        self.signature = *signature.as_bytes();
    }

    /// Returns the data to be signed.
    fn signing_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"BROADCAST:");
        data.extend_from_slice(&self.version.to_le_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.extend_from_slice(&(self.active_devices.len() as u32).to_le_bytes());
        for device in &self.active_devices {
            data.extend_from_slice(&device.device_id);
            data.extend_from_slice(&device.exchange_public_key);
        }
        data
    }

    /// Serializes to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Broadcast serialization should not fail")
    }

    /// Deserializes from JSON.
    pub fn from_json(json: &str) -> Result<Self, DeviceError> {
        serde_json::from_str(json).map_err(|_| DeviceError::InvalidRegistrySignature)
    }
}
