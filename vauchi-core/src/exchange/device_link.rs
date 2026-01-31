// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device Linking Protocol
//!
//! Enables linking multiple devices to the same identity via QR code scanning.
//! The existing device generates a QR containing a link key, the new device
//! scans it and receives the encrypted master seed to derive identical keys.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ring::rand::SystemRandom;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroize;

use super::ExchangeError;
use crate::crypto::{decrypt, encrypt, PublicKey, Signature, SymmetricKey};
use crate::identity::{DeviceInfo, DeviceRegistry, Identity};

/// QR code magic bytes for device linking.
const DEVICE_LINK_MAGIC: &[u8; 4] = b"WBDL";

/// Protocol version for device linking.
const DEVICE_LINK_VERSION: u8 = 1;

/// Link QR expiration time in seconds (5 minutes).
const LINK_QR_EXPIRY_SECONDS: u64 = 300;

/// Device link QR code data structure.
///
/// Displayed on the existing device for a new device to scan.
/// Contains a random link key used to encrypt the seed transfer.
#[derive(Clone, Debug)]
pub struct DeviceLinkQR {
    /// Protocol version
    version: u8,
    /// Identity's Ed25519 public key (so new device knows which identity)
    identity_public_key: [u8; 32],
    /// Random link key for encrypting the seed transfer (32 bytes)
    link_key: [u8; 32],
    /// Unix timestamp when QR was generated
    timestamp: u64,
    /// Signature over the above fields (proves identity ownership)
    signature: [u8; 64],
}

impl DeviceLinkQR {
    /// Generates a new device link QR code for the given identity.
    pub fn generate(identity: &Identity) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        Self::generate_with_timestamp(identity, timestamp)
    }

    /// Generates a link QR with a specific timestamp (for testing).
    pub fn generate_with_timestamp(identity: &Identity, timestamp: u64) -> Self {
        let rng = SystemRandom::new();

        // Generate random link key
        let link_key = ring::rand::generate::<[u8; 32]>(&rng)
            .expect("RNG should not fail")
            .expose();

        let identity_public_key = *identity.signing_public_key();

        // Create message to sign
        let mut message = Vec::new();
        message.push(DEVICE_LINK_VERSION);
        message.extend_from_slice(&identity_public_key);
        message.extend_from_slice(&link_key);
        message.extend_from_slice(&timestamp.to_be_bytes());

        // Sign the message
        let signature = identity.sign(&message);

        DeviceLinkQR {
            version: DEVICE_LINK_VERSION,
            identity_public_key,
            link_key,
            timestamp,
            signature: *signature.as_bytes(),
        }
    }

    /// Returns the identity public key.
    pub fn identity_public_key(&self) -> &[u8; 32] {
        &self.identity_public_key
    }

    /// Returns the link key (used for encrypting seed transfer).
    pub fn link_key(&self) -> &[u8; 32] {
        &self.link_key
    }

    /// Returns the timestamp.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Checks if the QR code has expired.
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        now > self.timestamp + LINK_QR_EXPIRY_SECONDS
    }

    /// Verifies the signature on the QR code.
    pub fn verify_signature(&self) -> bool {
        let mut message = Vec::new();
        message.push(self.version);
        message.extend_from_slice(&self.identity_public_key);
        message.extend_from_slice(&self.link_key);
        message.extend_from_slice(&self.timestamp.to_be_bytes());

        let public_key = PublicKey::from_bytes(self.identity_public_key);
        let signature = Signature::from_bytes(self.signature);

        public_key.verify(&message, &signature)
    }

    /// Encodes the QR data to a string for embedding in QR code.
    pub fn to_data_string(&self) -> String {
        // Format: base64(MAGIC || version || identity_key || link_key || timestamp || signature)
        let mut data = Vec::new();
        data.extend_from_slice(DEVICE_LINK_MAGIC);
        data.push(self.version);
        data.extend_from_slice(&self.identity_public_key);
        data.extend_from_slice(&self.link_key);
        data.extend_from_slice(&self.timestamp.to_be_bytes());
        data.extend_from_slice(&self.signature);

        BASE64.encode(&data)
    }

    /// Parses QR data from a scanned string.
    pub fn from_data_string(data: &str) -> Result<Self, ExchangeError> {
        let bytes = BASE64
            .decode(data)
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        // Minimum length: MAGIC(4) + version(1) + identity_key(32) + link_key(32) + timestamp(8) + sig(64) = 141
        if bytes.len() < 141 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        // Check magic bytes
        if &bytes[0..4] != DEVICE_LINK_MAGIC {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let version = bytes[4];
        if version != DEVICE_LINK_VERSION {
            return Err(ExchangeError::InvalidProtocolVersion);
        }

        let identity_public_key: [u8; 32] = bytes[5..37]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let link_key: [u8; 32] = bytes[37..69]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let timestamp = u64::from_be_bytes(
            bytes[69..77]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        );

        let signature: [u8; 64] = bytes[77..141]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let qr = DeviceLinkQR {
            version,
            identity_public_key,
            link_key,
            timestamp,
            signature,
        };

        // Verify signature
        if !qr.verify_signature() {
            return Err(ExchangeError::InvalidSignature);
        }

        Ok(qr)
    }

    /// Generates an actual QR code image as a string representation.
    pub fn to_qr_image_string(&self) -> String {
        use qrcode::QrCode;

        let data = self.to_data_string();
        let code = QrCode::new(&data).expect("QR generation should not fail");

        code.render()
            .light_color(' ')
            .dark_color('█')
            .quiet_zone(false)
            .build()
    }
}

impl Drop for DeviceLinkQR {
    fn drop(&mut self) {
        self.link_key.zeroize();
    }
}

/// Request from new device to link with existing identity.
#[derive(Clone, Debug)]
pub struct DeviceLinkRequest {
    /// New device's proposed name
    pub device_name: String,
    /// Random nonce to prevent replay attacks
    pub nonce: [u8; 32],
    /// Timestamp of request
    pub timestamp: u64,
}

impl DeviceLinkRequest {
    /// Creates a new device link request.
    pub fn new(device_name: String) -> Self {
        let rng = SystemRandom::new();
        let nonce = ring::rand::generate::<[u8; 32]>(&rng)
            .expect("RNG should not fail")
            .expose();

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        DeviceLinkRequest {
            device_name,
            nonce,
            timestamp,
        }
    }

    /// Serializes the request for transmission.
    pub fn to_bytes(&self) -> Vec<u8> {
        let name_bytes = self.device_name.as_bytes();
        let name_len = (name_bytes.len() as u32).to_le_bytes();

        let mut data = Vec::new();
        data.extend_from_slice(&name_len);
        data.extend_from_slice(name_bytes);
        data.extend_from_slice(&self.nonce);
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data
    }

    /// Deserializes a request from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, ExchangeError> {
        if data.len() < 4 + 32 + 8 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let name_len = u32::from_le_bytes(
            data[..4]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        ) as usize;

        if data.len() < 4 + name_len + 32 + 8 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let device_name = String::from_utf8(data[4..4 + name_len].to_vec())
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let nonce: [u8; 32] = data[4 + name_len..4 + name_len + 32]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let timestamp = u64::from_le_bytes(
            data[4 + name_len + 32..4 + name_len + 40]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        );

        Ok(DeviceLinkRequest {
            device_name,
            nonce,
            timestamp,
        })
    }

    /// Encrypts the request using the link key from the QR.
    pub fn encrypt(&self, link_key: &[u8; 32]) -> Result<Vec<u8>, ExchangeError> {
        let key = SymmetricKey::from_bytes(*link_key);
        let plaintext = self.to_bytes();
        encrypt(&key, &plaintext).map_err(|_| ExchangeError::CryptoError)
    }

    /// Decrypts a request using the link key.
    pub fn decrypt(ciphertext: &[u8], link_key: &[u8; 32]) -> Result<Self, ExchangeError> {
        let key = SymmetricKey::from_bytes(*link_key);
        let plaintext = decrypt(&key, ciphertext).map_err(|_| ExchangeError::CryptoError)?;
        Self::from_bytes(&plaintext)
    }
}

/// Response from existing device containing the encrypted seed.
#[derive(Clone)]
pub struct DeviceLinkResponse {
    /// The master seed (encrypted with link key before transmission)
    master_seed: [u8; 32],
    /// Identity display name
    display_name: String,
    /// Assigned device index for the new device
    device_index: u32,
    /// Current device registry
    registry: DeviceRegistry,
    /// Sync payload containing contacts and card (optional, may be empty).
    sync_payload_json: String,
}

impl DeviceLinkResponse {
    /// Creates a new device link response.
    ///
    /// The existing device creates this with its master seed and the next
    /// available device index.
    pub fn new(
        master_seed: [u8; 32],
        display_name: String,
        device_index: u32,
        registry: DeviceRegistry,
    ) -> Self {
        DeviceLinkResponse {
            master_seed,
            display_name,
            device_index,
            registry,
            sync_payload_json: String::new(),
        }
    }

    /// Creates a new device link response with sync payload.
    pub fn with_sync_payload(
        master_seed: [u8; 32],
        display_name: String,
        device_index: u32,
        registry: DeviceRegistry,
        sync_payload_json: String,
    ) -> Self {
        DeviceLinkResponse {
            master_seed,
            display_name,
            device_index,
            registry,
            sync_payload_json,
        }
    }

    /// Returns the master seed.
    pub fn master_seed(&self) -> &[u8; 32] {
        &self.master_seed
    }

    /// Returns the display name.
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns the device index.
    pub fn device_index(&self) -> u32 {
        self.device_index
    }

    /// Returns the device registry.
    pub fn registry(&self) -> &DeviceRegistry {
        &self.registry
    }

    /// Returns the sync payload JSON (may be empty).
    pub fn sync_payload_json(&self) -> &str {
        &self.sync_payload_json
    }

    /// Serializes the response for transmission.
    pub fn to_bytes(&self) -> Vec<u8> {
        let name_bytes = self.display_name.as_bytes();
        let name_len = (name_bytes.len() as u32).to_le_bytes();
        let registry_json = self.registry.to_json();
        let registry_bytes = registry_json.as_bytes();
        let registry_len = (registry_bytes.len() as u32).to_le_bytes();
        let sync_bytes = self.sync_payload_json.as_bytes();
        let sync_len = (sync_bytes.len() as u32).to_le_bytes();

        let mut data = Vec::new();
        data.extend_from_slice(&self.master_seed);
        data.extend_from_slice(&name_len);
        data.extend_from_slice(name_bytes);
        data.extend_from_slice(&self.device_index.to_le_bytes());
        data.extend_from_slice(&registry_len);
        data.extend_from_slice(registry_bytes);
        data.extend_from_slice(&sync_len);
        data.extend_from_slice(sync_bytes);
        data
    }

    /// Deserializes a response from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, ExchangeError> {
        if data.len() < 32 + 4 + 4 + 4 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let master_seed: [u8; 32] = data[..32]
            .try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let name_len = u32::from_le_bytes(
            data[32..36]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        ) as usize;

        if data.len() < 32 + 4 + name_len + 4 + 4 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let display_name = String::from_utf8(data[36..36 + name_len].to_vec())
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let offset = 36 + name_len;
        let device_index = u32::from_le_bytes(
            data[offset..offset + 4]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        );

        let registry_len = u32::from_le_bytes(
            data[offset + 4..offset + 8]
                .try_into()
                .map_err(|_| ExchangeError::InvalidQRFormat)?,
        ) as usize;

        if data.len() < offset + 8 + registry_len {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let registry_json = String::from_utf8(data[offset + 8..offset + 8 + registry_len].to_vec())
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let registry = DeviceRegistry::from_json(&registry_json)
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        // Parse sync payload (optional, may be empty or missing in older formats)
        let sync_offset = offset + 8 + registry_len;
        let sync_payload_json = if data.len() >= sync_offset + 4 {
            let sync_len = u32::from_le_bytes(
                data[sync_offset..sync_offset + 4]
                    .try_into()
                    .map_err(|_| ExchangeError::InvalidQRFormat)?,
            ) as usize;

            if data.len() >= sync_offset + 4 + sync_len {
                String::from_utf8(data[sync_offset + 4..sync_offset + 4 + sync_len].to_vec())
                    .map_err(|_| ExchangeError::InvalidQRFormat)?
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        Ok(DeviceLinkResponse {
            master_seed,
            display_name,
            device_index,
            registry,
            sync_payload_json,
        })
    }

    /// Encrypts the response using the link key.
    pub fn encrypt(&self, link_key: &[u8; 32]) -> Result<Vec<u8>, ExchangeError> {
        let key = SymmetricKey::from_bytes(*link_key);
        let plaintext = self.to_bytes();
        encrypt(&key, &plaintext).map_err(|_| ExchangeError::CryptoError)
    }

    /// Decrypts a response using the link key.
    pub fn decrypt(ciphertext: &[u8], link_key: &[u8; 32]) -> Result<Self, ExchangeError> {
        let key = SymmetricKey::from_bytes(*link_key);
        let plaintext = decrypt(&key, ciphertext).map_err(|_| ExchangeError::CryptoError)?;
        Self::from_bytes(&plaintext)
    }
}

impl Drop for DeviceLinkResponse {
    fn drop(&mut self) {
        self.master_seed.zeroize();
    }
}

/// State machine for device linking from the existing device's perspective.
pub struct DeviceLinkInitiator {
    /// The identity on this device (reserved for future verification)
    _identity_public_key: [u8; 32],
    /// Master seed to transfer (kept for creating response)
    master_seed: [u8; 32],
    /// Display name to transfer
    display_name: String,
    /// The generated QR code
    qr: DeviceLinkQR,
    /// Current device registry
    registry: DeviceRegistry,
}

impl DeviceLinkInitiator {
    /// Creates a new device link initiator.
    ///
    /// The identity parameter is used to get the master seed for transfer.
    /// In a real implementation, we'd need a way to access the master seed
    /// from the identity - this is intentionally designed to require explicit
    /// seed access for security.
    pub fn new(master_seed: [u8; 32], identity: &Identity, registry: DeviceRegistry) -> Self {
        let qr = DeviceLinkQR::generate(identity);

        DeviceLinkInitiator {
            _identity_public_key: *identity.signing_public_key(),
            master_seed,
            display_name: identity.display_name().to_string(),
            qr,
            registry,
        }
    }

    /// Returns the QR code to display.
    pub fn qr(&self) -> &DeviceLinkQR {
        &self.qr
    }

    /// Processes a link request and creates a response.
    ///
    /// Returns the encrypted response and the updated registry with the new device.
    pub fn process_request(
        &self,
        encrypted_request: &[u8],
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        // Decrypt the request
        let request = DeviceLinkRequest::decrypt(encrypted_request, self.qr.link_key())?;

        // Validate device name
        if request.device_name.is_empty() {
            return Err(ExchangeError::InvalidQRFormat);
        }

        // Determine next device index
        let device_index = self.registry.next_device_index();

        // Create device info for the new device
        let new_device_info =
            DeviceInfo::derive(&self.master_seed, device_index, request.device_name.clone());

        // Create updated registry with new device
        let mut updated_registry = self.registry.clone();
        updated_registry
            .add_device_unsigned(new_device_info.to_registered(&self.master_seed))
            .map_err(|_| ExchangeError::CryptoError)?;

        // Create and encrypt response
        let response = DeviceLinkResponse::new(
            self.master_seed,
            self.display_name.clone(),
            device_index,
            updated_registry.clone(),
        );

        let encrypted_response = response.encrypt(self.qr.link_key())?;

        // Create the new device's DeviceInfo for the caller to store
        let new_device = DeviceInfo::derive(&self.master_seed, device_index, request.device_name);

        Ok((encrypted_response, updated_registry, new_device))
    }

    /// Processes a link request and creates a response with sync payload.
    ///
    /// This variant includes the full sync payload for the new device.
    pub fn process_request_with_sync(
        &self,
        encrypted_request: &[u8],
        sync_payload_json: &str,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        // Decrypt the request
        let request = DeviceLinkRequest::decrypt(encrypted_request, self.qr.link_key())?;

        // Validate device name
        if request.device_name.is_empty() {
            return Err(ExchangeError::InvalidQRFormat);
        }

        // Determine next device index
        let device_index = self.registry.next_device_index();

        // Create device info for the new device
        let new_device_info =
            DeviceInfo::derive(&self.master_seed, device_index, request.device_name.clone());

        // Create updated registry with new device
        let mut updated_registry = self.registry.clone();
        updated_registry
            .add_device_unsigned(new_device_info.to_registered(&self.master_seed))
            .map_err(|_| ExchangeError::CryptoError)?;

        // Create response with sync payload
        let response = DeviceLinkResponse::with_sync_payload(
            self.master_seed,
            self.display_name.clone(),
            device_index,
            updated_registry.clone(),
            sync_payload_json.to_string(),
        );

        let encrypted_response = response.encrypt(self.qr.link_key())?;

        // Create the new device's DeviceInfo for the caller to store
        let new_device = DeviceInfo::derive(&self.master_seed, device_index, request.device_name);

        Ok((encrypted_response, updated_registry, new_device))
    }
}

impl Drop for DeviceLinkInitiator {
    fn drop(&mut self) {
        self.master_seed.zeroize();
    }
}

/// State machine for device linking from the existing device's perspective (restored from saved QR).
///
/// Used when the QR was generated earlier and saved to disk, then restored
/// when the request comes in.
pub struct DeviceLinkInitiatorRestored {
    /// Master seed to transfer
    master_seed: [u8; 32],
    /// Display name to transfer
    display_name: String,
    /// The restored QR code
    qr: DeviceLinkQR,
    /// Current device registry
    registry: DeviceRegistry,
}

impl DeviceLinkInitiatorRestored {
    /// Creates a restored initiator with a previously saved QR code.
    pub fn new(
        master_seed: [u8; 32],
        identity: &Identity,
        registry: DeviceRegistry,
        qr: DeviceLinkQR,
    ) -> Self {
        DeviceLinkInitiatorRestored {
            master_seed,
            display_name: identity.display_name().to_string(),
            qr,
            registry,
        }
    }

    /// Returns the QR code.
    pub fn qr(&self) -> &DeviceLinkQR {
        &self.qr
    }

    /// Processes a link request and creates a response.
    ///
    /// Returns the encrypted response and the updated registry with the new device.
    pub fn process_request(
        &self,
        encrypted_request: &[u8],
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        // Decrypt the request
        let request = DeviceLinkRequest::decrypt(encrypted_request, self.qr.link_key())?;

        // Validate device name
        if request.device_name.is_empty() {
            return Err(ExchangeError::InvalidQRFormat);
        }

        // Determine next device index
        let device_index = self.registry.next_device_index();

        // Create device info for the new device
        let new_device_info =
            DeviceInfo::derive(&self.master_seed, device_index, request.device_name.clone());

        // Create updated registry with new device
        let mut updated_registry = self.registry.clone();
        updated_registry
            .add_device_unsigned(new_device_info.to_registered(&self.master_seed))
            .map_err(|_| ExchangeError::CryptoError)?;

        // Create and encrypt response
        let response = DeviceLinkResponse::new(
            self.master_seed,
            self.display_name.clone(),
            device_index,
            updated_registry.clone(),
        );

        let encrypted_response = response.encrypt(self.qr.link_key())?;

        // Create the new device's DeviceInfo for the caller to store
        let new_device = DeviceInfo::derive(&self.master_seed, device_index, request.device_name);

        Ok((encrypted_response, updated_registry, new_device))
    }

    /// Processes a link request with sync payload and creates a response.
    ///
    /// This variant includes the sync payload in the response so the new device
    /// receives all existing contacts during initial linking.
    pub fn process_request_with_sync(
        &self,
        encrypted_request: &[u8],
        sync_payload_json: &str,
    ) -> Result<(Vec<u8>, DeviceRegistry, DeviceInfo), ExchangeError> {
        // Decrypt the request
        let request = DeviceLinkRequest::decrypt(encrypted_request, self.qr.link_key())?;

        // Validate device name
        if request.device_name.is_empty() {
            return Err(ExchangeError::InvalidQRFormat);
        }

        // Determine next device index
        let device_index = self.registry.next_device_index();

        // Create device info for the new device
        let new_device_info =
            DeviceInfo::derive(&self.master_seed, device_index, request.device_name.clone());

        // Create updated registry with new device
        let mut updated_registry = self.registry.clone();
        updated_registry
            .add_device_unsigned(new_device_info.to_registered(&self.master_seed))
            .map_err(|_| ExchangeError::CryptoError)?;

        // Create and encrypt response with sync payload
        let response = DeviceLinkResponse::with_sync_payload(
            self.master_seed,
            self.display_name.clone(),
            device_index,
            updated_registry.clone(),
            sync_payload_json.to_string(),
        );

        let encrypted_response = response.encrypt(self.qr.link_key())?;

        // Create the new device's DeviceInfo for the caller to store
        let new_device = DeviceInfo::derive(&self.master_seed, device_index, request.device_name);

        Ok((encrypted_response, updated_registry, new_device))
    }
}

impl Drop for DeviceLinkInitiatorRestored {
    fn drop(&mut self) {
        self.master_seed.zeroize();
    }
}

/// State machine for device linking from the new device's perspective.
pub struct DeviceLinkResponder {
    /// The scanned QR code
    qr: DeviceLinkQR,
    /// The device name for this new device
    device_name: String,
}

impl DeviceLinkResponder {
    /// Creates a new responder after scanning a device link QR.
    pub fn from_qr(qr: DeviceLinkQR, device_name: String) -> Result<Self, ExchangeError> {
        if qr.is_expired() {
            return Err(ExchangeError::TokenExpired);
        }

        Ok(DeviceLinkResponder { qr, device_name })
    }

    /// Creates a request to send to the existing device.
    pub fn create_request(&self) -> Result<Vec<u8>, ExchangeError> {
        let request = DeviceLinkRequest::new(self.device_name.clone());
        request.encrypt(self.qr.link_key())
    }

    /// Processes the response from the existing device.
    ///
    /// Returns the master seed, display name, device index, and registry.
    pub fn process_response(
        &self,
        encrypted_response: &[u8],
    ) -> Result<DeviceLinkResponse, ExchangeError> {
        DeviceLinkResponse::decrypt(encrypted_response, self.qr.link_key())
    }

    /// Returns the identity public key from the QR.
    pub fn identity_public_key(&self) -> &[u8; 32] {
        self.qr.identity_public_key()
    }
}

/// Generates a 6-digit numeric code from cryptographically random bytes.
///
/// This serves as a fallback pairing mechanism when QR code scanning is not
/// available (e.g., accessibility needs, camera failure). The code is derived
/// by taking 4 random bytes, interpreting them as a `u32`, and reducing
/// modulo 1_000_000 to produce a zero-padded 6-digit string.
pub fn generate_numeric_code() -> String {
    let rng = SystemRandom::new();
    let random_bytes = ring::rand::generate::<[u8; 4]>(&rng)
        .expect("RNG should not fail")
        .expose();

    let value = u32::from_le_bytes(random_bytes) % 1_000_000;
    format!("{value:06}")
}

// INLINE_TEST_REQUIRED: Tests private DEVICE_LINK_VERSION, DEVICE_LINK_MAGIC, BASE64 constants and version field
#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_identity() -> Identity {
        Identity::create("Test User")
    }

    fn create_test_registry(identity: &Identity) -> DeviceRegistry {
        let device_info = identity.device_info();
        let master_seed = [0x42u8; 32]; // Test seed
        DeviceRegistry::new(
            device_info.to_registered(&master_seed),
            identity.signing_keypair(),
        )
    }

    #[test]
    fn test_device_link_qr_generation() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);

        assert_eq!(qr.version, DEVICE_LINK_VERSION);
        assert_eq!(qr.identity_public_key(), identity.signing_public_key());
        assert!(!qr.is_expired());
    }

    #[test]
    fn test_device_link_qr_signature_valid() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);

        assert!(qr.verify_signature());
    }

    #[test]
    fn test_device_link_qr_roundtrip() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);

        let data_string = qr.to_data_string();
        let restored = DeviceLinkQR::from_data_string(&data_string).unwrap();

        assert_eq!(restored.identity_public_key(), qr.identity_public_key());
        assert_eq!(restored.link_key(), qr.link_key());
        assert_eq!(restored.timestamp(), qr.timestamp());
    }

    #[test]
    fn test_device_link_qr_expired() {
        let identity = create_test_identity();
        // Create QR with timestamp 20 minutes ago
        let old_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 1200;

        let qr = DeviceLinkQR::generate_with_timestamp(&identity, old_timestamp);
        assert!(qr.is_expired());
    }

    #[test]
    fn test_device_link_request_roundtrip() {
        let request = DeviceLinkRequest::new("My New Phone".to_string());
        let bytes = request.to_bytes();
        let restored = DeviceLinkRequest::from_bytes(&bytes).unwrap();

        assert_eq!(restored.device_name, request.device_name);
        assert_eq!(restored.nonce, request.nonce);
        assert_eq!(restored.timestamp, request.timestamp);
    }

    #[test]
    fn test_device_link_request_encryption() {
        let request = DeviceLinkRequest::new("My New Phone".to_string());
        let link_key = [0x42u8; 32];

        let encrypted = request.encrypt(&link_key).unwrap();
        let decrypted = DeviceLinkRequest::decrypt(&encrypted, &link_key).unwrap();

        assert_eq!(decrypted.device_name, request.device_name);
    }

    #[test]
    fn test_device_link_response_roundtrip() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let response =
            DeviceLinkResponse::new(master_seed, "Alice".to_string(), 1, registry.clone());

        let bytes = response.to_bytes();
        let restored = DeviceLinkResponse::from_bytes(&bytes).unwrap();

        assert_eq!(restored.master_seed(), &master_seed);
        assert_eq!(restored.display_name(), "Alice");
        assert_eq!(restored.device_index(), 1);
    }

    #[test]
    fn test_device_link_response_encryption() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let response = DeviceLinkResponse::new(master_seed, "Alice".to_string(), 1, registry);

        let link_key = [0x55u8; 32];
        let encrypted = response.encrypt(&link_key).unwrap();
        let decrypted = DeviceLinkResponse::decrypt(&encrypted, &link_key).unwrap();

        assert_eq!(decrypted.master_seed(), &master_seed);
        assert_eq!(decrypted.display_name(), "Alice");
        assert_eq!(decrypted.device_index(), 1);
    }

    #[test]
    fn test_device_link_full_flow() {
        // Existing device (Device A) setup
        let master_seed_a = [0x42u8; 32];
        let identity_a = Identity::create("Alice");
        let registry_a = create_test_registry(&identity_a);

        // Device A creates link initiator
        let initiator = DeviceLinkInitiator::new(master_seed_a, &identity_a, registry_a);
        let qr = initiator.qr();

        // New device (Device B) scans QR
        let qr_string = qr.to_data_string();
        let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

        // Device B creates request
        let encrypted_request = responder.create_request().unwrap();

        // Device A processes request and creates response
        let (encrypted_response, updated_registry, new_device) =
            initiator.process_request(&encrypted_request).unwrap();

        // Device B processes response
        let response = responder.process_response(&encrypted_response).unwrap();

        // Verify the new device got the correct seed
        assert_eq!(response.master_seed(), &master_seed_a);
        assert_eq!(response.display_name(), "Alice");
        assert_eq!(response.device_index(), 1); // Second device gets index 1

        // Verify the new device info is correct
        assert_eq!(new_device.device_name(), "My Phone");
        assert_eq!(new_device.device_index(), 1);

        // Verify the registry was updated
        assert_eq!(updated_registry.device_count(), 2);
    }

    #[test]
    fn test_device_link_qr_wrong_magic() {
        let data = BASE64.encode(b"XXXX01\x00\x00\x00");
        let result = DeviceLinkQR::from_data_string(&data);
        assert!(matches!(result, Err(ExchangeError::InvalidQRFormat)));
    }

    #[test]
    fn test_device_link_request_wrong_key() {
        let request = DeviceLinkRequest::new("My Phone".to_string());
        let correct_key = [0x42u8; 32];
        let wrong_key = [0x99u8; 32];

        let encrypted = request.encrypt(&correct_key).unwrap();
        let result = DeviceLinkRequest::decrypt(&encrypted, &wrong_key);

        assert!(result.is_err());
    }

    // ============================================================
    // Additional edge case tests (added for coverage)
    // ============================================================

    #[test]
    fn test_device_link_response_with_sync_payload() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let sync_payload = r#"{"contacts":[],"own_card_json":"{}","version":1}"#;
        let response = DeviceLinkResponse::with_sync_payload(
            master_seed,
            "Alice".to_string(),
            1,
            registry.clone(),
            sync_payload.to_string(),
        );

        assert_eq!(response.sync_payload_json(), sync_payload);

        // Test roundtrip preserves sync payload
        let bytes = response.to_bytes();
        let restored = DeviceLinkResponse::from_bytes(&bytes).unwrap();
        assert_eq!(restored.sync_payload_json(), sync_payload);
    }

    #[test]
    fn test_device_link_response_encryption_with_sync_payload() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let sync_payload = r#"{"contacts":[{"id":"test"}]}"#;
        let response = DeviceLinkResponse::with_sync_payload(
            master_seed,
            "Alice".to_string(),
            1,
            registry,
            sync_payload.to_string(),
        );

        let link_key = [0x55u8; 32];
        let encrypted = response.encrypt(&link_key).unwrap();
        let decrypted = DeviceLinkResponse::decrypt(&encrypted, &link_key).unwrap();

        assert_eq!(decrypted.sync_payload_json(), sync_payload);
    }

    #[test]
    fn test_device_link_responder_expired_qr() {
        let identity = create_test_identity();
        // Create QR with timestamp 20 minutes ago (expired)
        let old_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 1200;

        let qr = DeviceLinkQR::generate_with_timestamp(&identity, old_timestamp);
        let result = DeviceLinkResponder::from_qr(qr, "My Phone".to_string());

        assert!(matches!(result, Err(ExchangeError::TokenExpired)));
    }

    #[test]
    fn test_device_link_qr_invalid_base64() {
        let result = DeviceLinkQR::from_data_string("not valid base64!!!");
        assert!(matches!(result, Err(ExchangeError::InvalidQRFormat)));
    }

    #[test]
    fn test_device_link_qr_invalid_version() {
        // Create valid-looking data but with wrong version
        let mut data = Vec::new();
        data.extend_from_slice(DEVICE_LINK_MAGIC);
        data.push(99); // Wrong version
        data.extend_from_slice(&[0u8; 32]); // identity_key
        data.extend_from_slice(&[0u8; 32]); // link_key
        data.extend_from_slice(&0u64.to_be_bytes()); // timestamp
        data.extend_from_slice(&[0u8; 64]); // signature

        let encoded = BASE64.encode(&data);
        let result = DeviceLinkQR::from_data_string(&encoded);

        assert!(matches!(result, Err(ExchangeError::InvalidProtocolVersion)));
    }

    #[test]
    fn test_device_link_qr_truncated_data() {
        // Data too short
        let data = BASE64.encode(b"WBDL\x01short");
        let result = DeviceLinkQR::from_data_string(&data);

        assert!(matches!(result, Err(ExchangeError::InvalidQRFormat)));
    }

    #[test]
    fn test_device_link_qr_invalid_signature() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);

        // Corrupt the signature
        let mut data = Vec::new();
        data.extend_from_slice(DEVICE_LINK_MAGIC);
        data.push(qr.version);
        data.extend_from_slice(qr.identity_public_key());
        data.extend_from_slice(qr.link_key());
        data.extend_from_slice(&qr.timestamp().to_be_bytes());
        data.extend_from_slice(&[0xFFu8; 64]); // Invalid signature

        let encoded = BASE64.encode(&data);
        let result = DeviceLinkQR::from_data_string(&encoded);

        assert!(matches!(result, Err(ExchangeError::InvalidSignature)));
    }

    #[test]
    fn test_device_link_process_request_empty_device_name() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);
        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

        // Create a request with empty device name
        let request = DeviceLinkRequest {
            device_name: "".to_string(),
            nonce: [0u8; 32],
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        let encrypted = request.encrypt(initiator.qr().link_key()).unwrap();

        let result = initiator.process_request(&encrypted);
        assert!(matches!(result, Err(ExchangeError::InvalidQRFormat)));
    }

    #[test]
    fn test_device_link_request_truncated_bytes() {
        // Test with truncated data
        let result = DeviceLinkRequest::from_bytes(&[0u8; 10]);
        assert!(matches!(result, Err(ExchangeError::InvalidQRFormat)));
    }

    #[test]
    fn test_device_link_response_truncated_bytes() {
        // Test with truncated data
        let result = DeviceLinkResponse::from_bytes(&[0u8; 10]);
        assert!(matches!(result, Err(ExchangeError::InvalidQRFormat)));
    }

    #[test]
    fn test_device_link_response_wrong_key() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let response = DeviceLinkResponse::new(master_seed, "Alice".to_string(), 1, registry);

        let correct_key = [0x42u8; 32];
        let wrong_key = [0x99u8; 32];

        let encrypted = response.encrypt(&correct_key).unwrap();
        let result = DeviceLinkResponse::decrypt(&encrypted, &wrong_key);

        assert!(result.is_err());
    }

    #[test]
    fn test_device_link_qr_to_qr_image_string() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);

        let image_string = qr.to_qr_image_string();

        // Should produce a non-empty string with blocks
        assert!(!image_string.is_empty());
        assert!(image_string.contains('█') || image_string.contains(' '));
    }

    #[test]
    fn test_device_link_responder_identity_public_key() {
        let identity = create_test_identity();
        let qr = DeviceLinkQR::generate(&identity);
        let responder = DeviceLinkResponder::from_qr(qr, "My Phone".to_string()).unwrap();

        assert_eq!(
            responder.identity_public_key(),
            identity.signing_public_key()
        );
    }

    #[test]
    fn test_device_link_initiator_qr_accessor() {
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);
        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

        let qr = initiator.qr();
        assert_eq!(qr.identity_public_key(), identity.signing_public_key());
        assert!(!qr.is_expired());
    }

    // ============================================================
    // Phase 8: Device Linking with Sync Payload Tests (TDD)
    // ============================================================

    use crate::contact::Contact;
    use crate::contact_card::ContactCard;
    use crate::storage::Storage;
    use crate::sync::{DeviceSyncOrchestrator, DeviceSyncPayload};

    fn create_test_storage() -> Storage {
        Storage::in_memory(SymmetricKey::generate()).unwrap()
    }

    fn create_test_contact(name: &str) -> Contact {
        let public_key = [0x42u8; 32];
        let card = ContactCard::new(name);
        let shared_key = SymmetricKey::generate();
        Contact::from_exchange(public_key, card, shared_key)
    }

    #[test]
    fn test_device_link_with_full_sync_payload() {
        // Existing device (Device A) setup with data
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);
        let storage = create_test_storage();

        // Add some data to sync
        let contact = create_test_contact("Bob");
        storage.save_contact(&contact).unwrap();

        let mut own_card = ContactCard::new("Alice");
        let _ = own_card.add_field(crate::contact_card::ContactField::new(
            crate::contact_card::FieldType::Email,
            "email",
            "alice@example.com",
        ));
        storage.save_own_card(&own_card).unwrap();

        // Create orchestrator to generate sync payload
        let device_a = DeviceInfo::derive(&master_seed, 0, "Device A".to_string());
        let orchestrator = DeviceSyncOrchestrator::new(&storage, device_a, registry.clone());
        let sync_payload = orchestrator.create_full_sync_payload().unwrap();
        let sync_json = serde_json::to_string(&sync_payload).unwrap();

        // Create initiator with sync payload
        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry.clone());

        // New device scans QR
        let qr_string = initiator.qr().to_data_string();
        let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();

        // Device B creates request
        let encrypted_request = responder.create_request().unwrap();

        // Device A processes request with sync payload
        let (encrypted_response, _updated_registry, _new_device) = initiator
            .process_request_with_sync(&encrypted_request, &sync_json)
            .unwrap();

        // Device B processes response
        let response = responder.process_response(&encrypted_response).unwrap();

        // Verify sync payload is included
        assert!(!response.sync_payload_json().is_empty());

        // Parse and verify sync payload contents
        let received_payload: DeviceSyncPayload =
            serde_json::from_str(response.sync_payload_json()).unwrap();
        assert_eq!(received_payload.contact_count(), 1);
        assert!(!received_payload.own_card_json.is_empty());
    }

    #[test]
    fn test_new_device_applies_full_state() {
        // Create sync payload
        let contact = create_test_contact("Bob");
        let own_card = ContactCard::new("Alice");
        let payload = DeviceSyncPayload::new(&[contact], &own_card, 1);
        let payload_json = serde_json::to_string(&payload).unwrap();

        // New device receives and parses payload
        let received: DeviceSyncPayload = serde_json::from_str(&payload_json).unwrap();

        // Verify payload contents
        assert_eq!(received.contact_count(), 1);
        assert_eq!(received.version, 1);
    }

    #[test]
    fn test_device_link_initiator_restored_flow() {
        // Device A creates a QR and saves it
        let master_seed = [0x42u8; 32];
        let identity = Identity::create("Alice");
        let registry = create_test_registry(&identity);

        let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry.clone());
        let qr = initiator.qr().clone();
        let qr_string = qr.to_data_string();

        // Later, Device A restores the QR from saved string
        let restored_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let restored_initiator =
            DeviceLinkInitiatorRestored::new(master_seed, &identity, registry, restored_qr);

        // Device B scans the QR and creates request
        let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let responder = DeviceLinkResponder::from_qr(scanned_qr, "My Phone".to_string()).unwrap();
        let encrypted_request = responder.create_request().unwrap();

        // Device A processes request using restored initiator
        let (encrypted_response, updated_registry, new_device) = restored_initiator
            .process_request(&encrypted_request)
            .unwrap();

        // Device B processes response
        let response = responder.process_response(&encrypted_response).unwrap();

        // Verify the flow worked correctly
        assert_eq!(response.master_seed(), &master_seed);
        assert_eq!(response.display_name(), "Alice");
        assert_eq!(response.device_index(), 1);
        assert_eq!(new_device.device_name(), "My Phone");
        assert_eq!(updated_registry.device_count(), 2);
    }

    #[test]
    fn test_identity_device_link_helper_methods() {
        // Test the new Identity helper methods
        let identity = Identity::create("Alice");

        // Test initial_device_registry
        let registry = identity.initial_device_registry();
        assert_eq!(registry.device_count(), 1);

        // Test create_device_link_initiator
        let initiator = identity.create_device_link_initiator(registry.clone());
        assert!(!initiator.qr().is_expired());
        assert_eq!(
            initiator.qr().identity_public_key(),
            identity.signing_public_key()
        );

        // Test restore_device_link_initiator
        let qr_string = initiator.qr().to_data_string();
        let restored_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
        let _restored = identity.restore_device_link_initiator(registry, restored_qr);
    }
}
