//! Device Linking Protocol
//!
//! Enables linking multiple devices to the same identity via QR code scanning.
//! The existing device generates a QR containing a link key, the new device
//! scans it and receives the encrypted master seed to derive identical keys.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use ring::rand::SystemRandom;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::crypto::{PublicKey, Signature, SymmetricKey, encrypt, decrypt};
use crate::identity::{Identity, DeviceRegistry, DeviceInfo};
use super::ExchangeError;

/// QR code magic bytes for device linking.
const DEVICE_LINK_MAGIC: &[u8; 4] = b"WBDL";

/// Protocol version for device linking.
const DEVICE_LINK_VERSION: u8 = 1;

/// Link QR expiration time in seconds (10 minutes).
const LINK_QR_EXPIRY_SECONDS: u64 = 600;

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
        let bytes = BASE64.decode(data)
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

        let identity_public_key: [u8; 32] = bytes[5..37].try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let link_key: [u8; 32] = bytes[37..69].try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let timestamp = u64::from_be_bytes(
            bytes[69..77].try_into().map_err(|_| ExchangeError::InvalidQRFormat)?
        );

        let signature: [u8; 64] = bytes[77..141].try_into()
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
            data[..4].try_into().map_err(|_| ExchangeError::InvalidQRFormat)?
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
                .map_err(|_| ExchangeError::InvalidQRFormat)?
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

    /// Serializes the response for transmission.
    pub fn to_bytes(&self) -> Vec<u8> {
        let name_bytes = self.display_name.as_bytes();
        let name_len = (name_bytes.len() as u32).to_le_bytes();
        let registry_json = self.registry.to_json();
        let registry_bytes = registry_json.as_bytes();
        let registry_len = (registry_bytes.len() as u32).to_le_bytes();

        let mut data = Vec::new();
        data.extend_from_slice(&self.master_seed);
        data.extend_from_slice(&name_len);
        data.extend_from_slice(name_bytes);
        data.extend_from_slice(&self.device_index.to_le_bytes());
        data.extend_from_slice(&registry_len);
        data.extend_from_slice(registry_bytes);
        data
    }

    /// Deserializes a response from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, ExchangeError> {
        if data.len() < 32 + 4 + 4 + 4 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let master_seed: [u8; 32] = data[..32].try_into()
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let name_len = u32::from_le_bytes(
            data[32..36].try_into().map_err(|_| ExchangeError::InvalidQRFormat)?
        ) as usize;

        if data.len() < 32 + 4 + name_len + 4 + 4 {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let display_name = String::from_utf8(data[36..36 + name_len].to_vec())
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let offset = 36 + name_len;
        let device_index = u32::from_le_bytes(
            data[offset..offset + 4].try_into().map_err(|_| ExchangeError::InvalidQRFormat)?
        );

        let registry_len = u32::from_le_bytes(
            data[offset + 4..offset + 8].try_into().map_err(|_| ExchangeError::InvalidQRFormat)?
        ) as usize;

        if data.len() < offset + 8 + registry_len {
            return Err(ExchangeError::InvalidQRFormat);
        }

        let registry_json = String::from_utf8(data[offset + 8..offset + 8 + registry_len].to_vec())
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        let registry = DeviceRegistry::from_json(&registry_json)
            .map_err(|_| ExchangeError::InvalidQRFormat)?;

        Ok(DeviceLinkResponse {
            master_seed,
            display_name,
            device_index,
            registry,
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

/// State machine for device linking from the existing device's perspective.
pub struct DeviceLinkInitiator {
    /// The identity on this device
    identity_public_key: [u8; 32],
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
    pub fn new(
        master_seed: [u8; 32],
        identity: &Identity,
        registry: DeviceRegistry,
    ) -> Self {
        let qr = DeviceLinkQR::generate(identity);

        DeviceLinkInitiator {
            identity_public_key: *identity.signing_public_key(),
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
        let new_device_info = DeviceInfo::derive(&self.master_seed, device_index, request.device_name.clone());

        // Create updated registry with new device
        let mut updated_registry = self.registry.clone();
        updated_registry.add_device_unsigned(new_device_info.to_registered(&self.master_seed))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_identity() -> Identity {
        Identity::create("Test User")
    }

    fn create_test_registry(identity: &Identity) -> DeviceRegistry {
        let device_info = identity.device_info();
        let master_seed = [0x42u8; 32]; // Test seed
        DeviceRegistry::new(device_info.to_registered(&master_seed), identity.signing_keypair())
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
            .as_secs() - 1200;

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

        let response = DeviceLinkResponse::new(
            master_seed,
            "Alice".to_string(),
            1,
            registry.clone(),
        );

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

        let response = DeviceLinkResponse::new(
            master_seed,
            "Alice".to_string(),
            1,
            registry,
        );

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
}
