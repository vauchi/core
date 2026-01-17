//! Ultrasonic Audio Proximity Verification
//!
//! Uses ultrasonic audio signals (18-20 kHz) to verify physical proximity
//! between devices during contact exchange. This prevents remote QR code
//! scanning attacks where an attacker photographs a QR code from a distance.
//!
//! ## Protocol
//!
//! 1. Alice displays QR code containing a 16-byte `audio_challenge`
//! 2. Bob scans QR code and emits ultrasonic signal encoding the challenge
//! 3. Alice's device detects the signal and verifies the challenge
//! 4. Alice's device emits a signed response
//! 5. Bob verifies Alice's response
//! 6. Both devices confirm proximity and proceed with key exchange
//!
//! ## Signal Design
//!
//! - Carrier frequency: 18-20 kHz (above human hearing threshold)
//! - Modulation: FSK (Frequency Shift Keying) for robustness
//! - Data rate: ~100 bps (sufficient for 16-byte challenge in <2 seconds)
//! - Error correction: Simple checksum for detection

use super::{ProximityError, ProximityVerifier};
use std::time::Duration;

/// Configuration for ultrasonic audio verification.
#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// Base carrier frequency in Hz (default: 18500 Hz)
    pub carrier_frequency: u32,
    /// Frequency shift for FSK modulation in Hz (default: 200 Hz)
    pub frequency_shift: u32,
    /// Sample rate in Hz (default: 44100 Hz)
    pub sample_rate: u32,
    /// Minimum signal-to-noise ratio for detection (default: 10.0 dB)
    pub min_snr_db: f32,
    /// Maximum detection distance in meters (default: 3.0)
    pub max_distance_meters: f32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig {
            carrier_frequency: 18500,
            frequency_shift: 200,
            sample_rate: 44100,
            min_snr_db: 10.0,
            max_distance_meters: 3.0,
        }
    }
}

/// Represents device audio capabilities.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioCapability {
    /// Device supports full ultrasonic audio (speaker + microphone)
    Full,
    /// Device can only emit ultrasonic audio (no microphone)
    EmitOnly,
    /// Device can only receive ultrasonic audio (no speaker)
    ReceiveOnly,
    /// Device does not support ultrasonic audio
    None,
}

/// Ultrasonic audio proximity verifier.
///
/// This is a trait-based design that allows platform-specific implementations
/// (iOS AVAudioEngine, Android AudioRecord/AudioTrack, Desktop CPAL).
pub struct UltrasonicVerifier {
    config: AudioConfig,
    capability: AudioCapability,
    /// Platform-specific audio backend (injected)
    backend: Box<dyn AudioBackend>,
}

/// Platform-specific audio backend trait.
///
/// Implementations handle actual audio I/O on each platform.
pub trait AudioBackend: Send + Sync {
    /// Checks device audio capabilities for ultrasonic frequencies.
    fn check_capability(&self) -> AudioCapability;

    /// Emits an ultrasonic signal encoding the given data.
    fn emit_signal(&self, data: &[u8], config: &AudioConfig) -> Result<(), ProximityError>;

    /// Listens for an ultrasonic signal and returns decoded data.
    fn receive_signal(
        &self,
        timeout: Duration,
        config: &AudioConfig,
    ) -> Result<Vec<u8>, ProximityError>;

    /// Returns true if currently emitting or receiving.
    fn is_active(&self) -> bool;

    /// Stops any ongoing audio operation.
    fn stop(&self);
}

impl UltrasonicVerifier {
    /// Creates a new ultrasonic verifier with the given backend.
    pub fn new(backend: Box<dyn AudioBackend>) -> Self {
        let capability = backend.check_capability();
        UltrasonicVerifier {
            config: AudioConfig::default(),
            capability,
            backend,
        }
    }

    /// Creates a verifier with custom configuration.
    pub fn with_config(backend: Box<dyn AudioBackend>, config: AudioConfig) -> Self {
        let capability = backend.check_capability();
        UltrasonicVerifier {
            config,
            capability,
            backend,
        }
    }

    /// Returns the device's audio capability.
    pub fn capability(&self) -> &AudioCapability {
        &self.capability
    }

    /// Returns true if the device supports ultrasonic audio.
    pub fn is_supported(&self) -> bool {
        self.capability != AudioCapability::None
    }

    /// Encodes a challenge into a transmittable format with checksum.
    fn encode_challenge(challenge: &[u8; 16]) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(18);
        encoded.extend_from_slice(challenge);
        // Simple checksum: XOR of all bytes
        let checksum: u8 = challenge.iter().fold(0, |acc, &b| acc ^ b);
        encoded.push(checksum);
        // Length prefix for framing
        encoded.insert(0, 17); // 16 bytes + 1 checksum
        encoded
    }

    /// Decodes and verifies a received signal.
    fn decode_response(data: &[u8]) -> Option<[u8; 16]> {
        if data.len() < 18 {
            return None;
        }
        let len = data[0] as usize;
        if len != 17 || data.len() < 18 {
            return None;
        }
        let challenge_bytes = &data[1..17];
        let checksum = data[17];
        // Verify checksum
        let computed: u8 = challenge_bytes.iter().fold(0, |acc, &b| acc ^ b);
        if computed != checksum {
            return None;
        }
        let mut result = [0u8; 16];
        result.copy_from_slice(challenge_bytes);
        Some(result)
    }
}

impl ProximityVerifier for UltrasonicVerifier {
    fn emit_challenge(&self, challenge: &[u8; 16]) -> Result<(), ProximityError> {
        if self.capability == AudioCapability::None
            || self.capability == AudioCapability::ReceiveOnly
        {
            return Err(ProximityError::NotSupported);
        }
        let encoded = Self::encode_challenge(challenge);
        self.backend.emit_signal(&encoded, &self.config)
    }

    fn listen_for_response(&self, timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        if self.capability == AudioCapability::None || self.capability == AudioCapability::EmitOnly
        {
            return Err(ProximityError::NotSupported);
        }
        self.backend.receive_signal(timeout, &self.config)
    }

    fn verify_response(&self, challenge: &[u8; 16], response: &[u8]) -> bool {
        if let Some(decoded) = Self::decode_response(response) {
            // Response should echo the challenge
            &decoded == challenge
        } else {
            false
        }
    }
}

/// Mock audio backend for testing.
pub struct MockAudioBackend {
    capability: AudioCapability,
    /// Simulated received data
    receive_data: std::sync::Mutex<Option<Vec<u8>>>,
    /// Whether operations should succeed
    should_succeed: bool,
    /// Simulated latency
    latency_ms: u64,
}

impl MockAudioBackend {
    /// Creates a mock backend with full capability.
    pub fn new() -> Self {
        MockAudioBackend {
            capability: AudioCapability::Full,
            receive_data: std::sync::Mutex::new(None),
            should_succeed: true,
            latency_ms: 0,
        }
    }

    /// Creates a mock with specific capability.
    pub fn with_capability(capability: AudioCapability) -> Self {
        MockAudioBackend {
            capability,
            receive_data: std::sync::Mutex::new(None),
            should_succeed: true,
            latency_ms: 0,
        }
    }

    /// Creates a mock that fails operations.
    pub fn failing() -> Self {
        MockAudioBackend {
            capability: AudioCapability::Full,
            receive_data: std::sync::Mutex::new(None),
            should_succeed: false,
            latency_ms: 0,
        }
    }

    /// Sets the data that will be "received" during listen.
    pub fn set_receive_data(&self, data: Vec<u8>) {
        *self.receive_data.lock().unwrap() = Some(data);
    }

    /// Simulates receiving a valid challenge response.
    pub fn simulate_valid_response(&self, challenge: &[u8; 16]) {
        let encoded = UltrasonicVerifier::encode_challenge(challenge);
        self.set_receive_data(encoded);
    }
}

impl Default for MockAudioBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioBackend for MockAudioBackend {
    fn check_capability(&self) -> AudioCapability {
        self.capability.clone()
    }

    fn emit_signal(&self, _data: &[u8], _config: &AudioConfig) -> Result<(), ProximityError> {
        if self.should_succeed {
            if self.latency_ms > 0 {
                std::thread::sleep(Duration::from_millis(self.latency_ms));
            }
            Ok(())
        } else {
            Err(ProximityError::HardwareError("Mock emit failure".into()))
        }
    }

    fn receive_signal(
        &self,
        _timeout: Duration,
        _config: &AudioConfig,
    ) -> Result<Vec<u8>, ProximityError> {
        if !self.should_succeed {
            return Err(ProximityError::HardwareError("Mock receive failure".into()));
        }
        if self.latency_ms > 0 {
            std::thread::sleep(Duration::from_millis(self.latency_ms));
        }
        self.receive_data
            .lock()
            .unwrap()
            .clone()
            .ok_or(ProximityError::Timeout)
    }

    fn is_active(&self) -> bool {
        false
    }

    fn stop(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_config_defaults() {
        let config = AudioConfig::default();
        assert_eq!(config.carrier_frequency, 18500);
        assert_eq!(config.sample_rate, 44100);
        assert!(config.carrier_frequency > 18000); // Above human hearing
    }

    #[test]
    fn test_encode_decode_challenge() {
        let challenge = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let encoded = UltrasonicVerifier::encode_challenge(&challenge);

        // Should have length prefix + 16 bytes + checksum
        assert_eq!(encoded.len(), 18);
        assert_eq!(encoded[0], 17); // Length prefix

        let decoded = UltrasonicVerifier::decode_response(&encoded);
        assert_eq!(decoded, Some(challenge));
    }

    #[test]
    fn test_decode_rejects_corrupted_data() {
        let challenge = [1u8; 16];
        let mut encoded = UltrasonicVerifier::encode_challenge(&challenge);

        // Corrupt a byte
        encoded[5] ^= 0xFF;

        let decoded = UltrasonicVerifier::decode_response(&encoded);
        assert_eq!(decoded, None);
    }

    #[test]
    fn test_decode_rejects_short_data() {
        let short_data = vec![1, 2, 3];
        assert_eq!(UltrasonicVerifier::decode_response(&short_data), None);
    }

    #[test]
    fn test_ultrasonic_verifier_emit_and_verify() {
        let backend = MockAudioBackend::new();
        let challenge = [42u8; 16];
        backend.simulate_valid_response(&challenge);

        let verifier = UltrasonicVerifier::new(Box::new(backend));

        assert!(verifier.is_supported());
        assert_eq!(verifier.capability(), &AudioCapability::Full);

        // Should be able to emit
        assert!(verifier.emit_challenge(&challenge).is_ok());

        // Should receive valid response
        let response = verifier
            .listen_for_response(Duration::from_secs(5))
            .unwrap();
        assert!(verifier.verify_response(&challenge, &response));
    }

    #[test]
    fn test_ultrasonic_verifier_full_cycle() {
        let backend = MockAudioBackend::new();
        let challenge = [99u8; 16];
        backend.simulate_valid_response(&challenge);

        let verifier = UltrasonicVerifier::new(Box::new(backend));

        let result = verifier.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(result.is_ok());
    }

    #[test]
    fn test_ultrasonic_verifier_wrong_response() {
        let backend = MockAudioBackend::new();
        let challenge = [1u8; 16];
        let wrong_challenge = [2u8; 16];
        backend.simulate_valid_response(&wrong_challenge);

        let verifier = UltrasonicVerifier::new(Box::new(backend));

        let result = verifier.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(matches!(result, Err(ProximityError::InvalidResponse)));
    }

    #[test]
    fn test_ultrasonic_verifier_timeout() {
        let backend = MockAudioBackend::new();
        // Don't set any receive data - will timeout

        let verifier = UltrasonicVerifier::new(Box::new(backend));

        let result = verifier.verify_proximity(&[0u8; 16], Duration::from_secs(1));
        assert!(matches!(result, Err(ProximityError::Timeout)));
    }

    #[test]
    fn test_ultrasonic_verifier_emit_only_device() {
        let backend = MockAudioBackend::with_capability(AudioCapability::EmitOnly);
        let verifier = UltrasonicVerifier::new(Box::new(backend));

        // Can emit
        assert!(verifier.emit_challenge(&[0u8; 16]).is_ok());

        // Cannot receive
        let result = verifier.listen_for_response(Duration::from_secs(1));
        assert!(matches!(result, Err(ProximityError::NotSupported)));
    }

    #[test]
    fn test_ultrasonic_verifier_receive_only_device() {
        let backend = MockAudioBackend::with_capability(AudioCapability::ReceiveOnly);
        let challenge = [0u8; 16];
        backend.simulate_valid_response(&challenge);

        let verifier = UltrasonicVerifier::new(Box::new(backend));

        // Cannot emit
        let result = verifier.emit_challenge(&challenge);
        assert!(matches!(result, Err(ProximityError::NotSupported)));

        // Can receive
        assert!(verifier.listen_for_response(Duration::from_secs(1)).is_ok());
    }

    #[test]
    fn test_ultrasonic_verifier_unsupported_device() {
        let backend = MockAudioBackend::with_capability(AudioCapability::None);
        let verifier = UltrasonicVerifier::new(Box::new(backend));

        assert!(!verifier.is_supported());

        let result = verifier.emit_challenge(&[0u8; 16]);
        assert!(matches!(result, Err(ProximityError::NotSupported)));
    }

    #[test]
    fn test_ultrasonic_verifier_hardware_failure() {
        let backend = MockAudioBackend::failing();
        let verifier = UltrasonicVerifier::new(Box::new(backend));

        let result = verifier.emit_challenge(&[0u8; 16]);
        assert!(matches!(result, Err(ProximityError::HardwareError(_))));
    }
}
