//! Device Management Module
//!
//! Handles multi-device support for WebBook identities.
//! Each device gets unique communication keys derived from the master seed.

use crate::crypto::{HKDF, SigningKeyPair, Signature};
use crate::exchange::X3DHKeyPair;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Domain separation constants for device key derivation.
const DEVICE_ID_INFO: &[u8] = b"WebBook_Device_ID";
const DEVICE_EXCHANGE_INFO: &[u8] = b"WebBook_Device_Exchange";

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

        // Derive device ID: HKDF(master_seed, index, "WebBook_Device_ID")
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
        bytes.try_into().map_err(|_| serde::de::Error::custom("invalid signature length"))
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

        let device = self.devices
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
                .as_secs()
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
        self.devices
            .iter()
            .map(|_| 1u32)
            .sum::<u32>()
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
        let device = self.devices
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
    pub fn create(
        device_id: &[u8; 32],
        reason: String,
        signing_key: &SigningKeyPair,
    ) -> Self {
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
        self.active_devices.iter().any(|d| &d.device_id == device_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_master_seed() -> [u8; 32] {
        [0x42u8; 32]
    }

    fn test_signing_keypair() -> SigningKeyPair {
        SigningKeyPair::from_seed(&test_master_seed())
    }

    #[test]
    fn test_device_key_derivation_is_deterministic() {
        let seed = test_master_seed();

        let device1 = DeviceInfo::derive(&seed, 0, "Device 1".to_string());
        let device2 = DeviceInfo::derive(&seed, 0, "Device 1".to_string());

        assert_eq!(device1.device_id(), device2.device_id());
        assert_eq!(device1.exchange_public_key(), device2.exchange_public_key());
    }

    #[test]
    fn test_different_index_different_keys() {
        let seed = test_master_seed();

        let device0 = DeviceInfo::derive(&seed, 0, "Device 0".to_string());
        let device1 = DeviceInfo::derive(&seed, 1, "Device 1".to_string());

        assert_ne!(device0.device_id(), device1.device_id());
        assert_ne!(device0.exchange_public_key(), device1.exchange_public_key());
    }

    #[test]
    fn test_different_seed_different_keys() {
        let seed1 = [0x42u8; 32];
        let seed2 = [0x43u8; 32];

        let device1 = DeviceInfo::derive(&seed1, 0, "Device".to_string());
        let device2 = DeviceInfo::derive(&seed2, 0, "Device".to_string());

        assert_ne!(device1.device_id(), device2.device_id());
        assert_ne!(device1.exchange_public_key(), device2.exchange_public_key());
    }

    #[test]
    fn test_device_registry_creation() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

        let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);

        assert_eq!(registry.version(), 1);
        assert_eq!(registry.active_count(), 1);
        assert!(registry.verify(&signing_key.public_key()));
    }

    #[test]
    fn test_add_device_to_registry() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());
        let device1 = DeviceInfo::derive(&seed, 1, "Secondary".to_string());

        let mut registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);
        registry.add_device(device1.to_registered(&seed), &signing_key).unwrap();

        assert_eq!(registry.version(), 2);
        assert_eq!(registry.active_count(), 2);
        assert!(registry.verify(&signing_key.public_key()));
    }

    #[test]
    fn test_max_devices_limit() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device0 = DeviceInfo::derive(&seed, 0, "Device 0".to_string());

        let mut registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);

        // Add devices up to limit
        for i in 1..MAX_DEVICES {
            let device = DeviceInfo::derive(&seed, i as u32, format!("Device {}", i));
            registry.add_device(device.to_registered(&seed), &signing_key).unwrap();
        }

        assert_eq!(registry.active_count(), MAX_DEVICES);

        // Adding one more should fail
        let extra = DeviceInfo::derive(&seed, MAX_DEVICES as u32, "Extra".to_string());
        let result = registry.add_device(extra.to_registered(&seed), &signing_key);
        assert!(matches!(result, Err(DeviceError::MaxDevicesReached)));
    }

    #[test]
    fn test_revoke_device() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());
        let device1 = DeviceInfo::derive(&seed, 1, "Secondary".to_string());

        let mut registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);
        registry.add_device(device1.to_registered(&seed), &signing_key).unwrap();

        assert_eq!(registry.active_count(), 2);

        registry.revoke_device(device1.device_id(), &signing_key).unwrap();

        assert_eq!(registry.active_count(), 1);
        assert_eq!(registry.all_devices().len(), 2); // Still in registry, just revoked
        assert!(registry.verify(&signing_key.public_key()));
    }

    #[test]
    fn test_cannot_revoke_last_device() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device = DeviceInfo::derive(&seed, 0, "Only Device".to_string());

        let mut registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);

        let result = registry.revoke_device(device.device_id(), &signing_key);
        assert!(matches!(result, Err(DeviceError::CannotRemoveLastDevice)));
    }

    #[test]
    fn test_find_device() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());
        let device_id = *device.device_id();

        let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);

        let found = registry.find_device(&device_id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().device_name, "Primary");

        let not_found = registry.find_device(&[0u8; 32]);
        assert!(not_found.is_none());
    }

    #[test]
    fn test_duplicate_device_rejected() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

        let mut registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);

        let result = registry.add_device(device.to_registered(&seed), &signing_key);
        assert!(matches!(result, Err(DeviceError::DeviceAlreadyExists)));
    }

    #[test]
    fn test_registry_serialization() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

        let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);

        let json = serde_json::to_string(&registry).unwrap();
        let restored: DeviceRegistry = serde_json::from_str(&json).unwrap();

        assert_eq!(registry.version(), restored.version());
        assert_eq!(registry.active_count(), restored.active_count());
        assert!(restored.verify(&signing_key.public_key()));
    }

    #[test]
    fn test_empty_device_name_rejected() {
        let seed = test_master_seed();
        let mut device = DeviceInfo::derive(&seed, 0, "Valid".to_string());

        let result = device.set_device_name("".to_string());
        assert!(matches!(result, Err(DeviceError::EmptyDeviceName)));
    }

    // ============================================================
    // Phase 5 Tests: Device Revocation
    // Based on features/device_management.feature @unlink and @security
    // ============================================================

    /// Scenario: Unlink a device remotely
    /// "Device B should no longer receive updates"
    /// "Device B should be notified of removal"
    #[test]
    fn test_device_revocation_certificate_creation() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device = DeviceInfo::derive(&seed, 1, "Lost Device".to_string());

        // Create a revocation certificate
        let certificate = DeviceRevocationCertificate::create(
            device.device_id(),
            "Lost device - reported stolen".to_string(),
            &signing_key,
        );

        assert_eq!(certificate.device_id(), device.device_id());
        assert!(certificate.verify(&signing_key.public_key()));
    }

    /// Scenario: Lost device revocation
    /// "Device B's device key should be revoked"
    #[test]
    fn test_device_revocation_certificate_has_timestamp() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device = DeviceInfo::derive(&seed, 1, "Lost Device".to_string());

        let certificate = DeviceRevocationCertificate::create(
            device.device_id(),
            "Lost".to_string(),
            &signing_key,
        );

        // Certificate should have valid timestamp
        assert!(certificate.revoked_at() > 0);
    }

    /// Test certificate serialization for transmission
    #[test]
    fn test_device_revocation_certificate_serialization() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device = DeviceInfo::derive(&seed, 1, "Lost Device".to_string());

        let certificate = DeviceRevocationCertificate::create(
            device.device_id(),
            "Lost".to_string(),
            &signing_key,
        );

        let json = certificate.to_json();
        let restored = DeviceRevocationCertificate::from_json(&json).unwrap();

        assert_eq!(certificate.device_id(), restored.device_id());
        assert!(restored.verify(&signing_key.public_key()));
    }

    /// Scenario: contacts should be notified if necessary
    #[test]
    fn test_registry_broadcast_message_creation() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

        let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);

        // Create broadcast message for contacts
        let broadcast = RegistryBroadcast::new(&registry, &signing_key);

        assert_eq!(broadcast.version(), registry.version());
        assert!(broadcast.verify(&signing_key.public_key()));
    }

    /// Test registry broadcast includes active device keys
    #[test]
    fn test_registry_broadcast_contains_active_devices() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());
        let device1 = DeviceInfo::derive(&seed, 1, "Secondary".to_string());

        let mut registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);
        registry.add_device(device1.to_registered(&seed), &signing_key).unwrap();

        let broadcast = RegistryBroadcast::new(&registry, &signing_key);

        assert_eq!(broadcast.active_device_count(), 2);
        assert!(broadcast.contains_device(device0.device_id()));
        assert!(broadcast.contains_device(device1.device_id()));
    }

    /// Test registry broadcast excludes revoked devices
    #[test]
    fn test_registry_broadcast_excludes_revoked() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());
        let device1 = DeviceInfo::derive(&seed, 1, "Revoked".to_string());

        let mut registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);
        registry.add_device(device1.to_registered(&seed), &signing_key).unwrap();
        registry.revoke_device(device1.device_id(), &signing_key).unwrap();

        let broadcast = RegistryBroadcast::new(&registry, &signing_key);

        assert_eq!(broadcast.active_device_count(), 1);
        assert!(broadcast.contains_device(device0.device_id()));
        assert!(!broadcast.contains_device(device1.device_id()));
    }

    /// Test registry broadcast serialization for transmission
    #[test]
    fn test_registry_broadcast_serialization() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device = DeviceInfo::derive(&seed, 0, "Primary".to_string());

        let registry = DeviceRegistry::new(device.to_registered(&seed), &signing_key);
        let broadcast = RegistryBroadcast::new(&registry, &signing_key);

        let json = broadcast.to_json();
        let restored = RegistryBroadcast::from_json(&json).unwrap();

        assert_eq!(broadcast.version(), restored.version());
        assert!(restored.verify(&signing_key.public_key()));
    }

    /// Test applying revocation certificate to local knowledge of contact
    #[test]
    fn test_apply_revocation_to_contact_registry() {
        let seed = test_master_seed();
        let signing_key = test_signing_keypair();
        let device0 = DeviceInfo::derive(&seed, 0, "Primary".to_string());
        let device1 = DeviceInfo::derive(&seed, 1, "ToRevoke".to_string());

        let mut registry = DeviceRegistry::new(device0.to_registered(&seed), &signing_key);
        registry.add_device(device1.to_registered(&seed), &signing_key).unwrap();

        // Create revocation certificate for device1
        let certificate = DeviceRevocationCertificate::create(
            device1.device_id(),
            "Revoked".to_string(),
            &signing_key,
        );

        // Apply certificate to registry (as if received from contact)
        registry.apply_revocation(&certificate, &signing_key.public_key()).unwrap();

        assert_eq!(registry.active_count(), 1);
        assert!(!registry.find_device(device1.device_id()).unwrap().is_active());
    }
}
