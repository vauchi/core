//! Proximity Verification
//!
//! Trait-based proximity verification to prevent remote QR code scanning attacks.
//! Implementations can use ultrasonic audio, BLE, NFC, or other mechanisms.

use std::time::Duration;
use thiserror::Error;

/// Errors that can occur during proximity verification.
#[derive(Error, Debug)]
pub enum ProximityError {
    #[error("Proximity verification timed out")]
    Timeout,

    #[error("No response received")]
    NoResponse,

    #[error("Invalid response")]
    InvalidResponse,

    #[error("Device not supported")]
    NotSupported,

    #[error("Device is too far away")]
    TooFar,

    #[error("Hardware error: {0}")]
    HardwareError(String),

    #[error("Device error: {0}")]
    DeviceError(String),
}

/// Trait for proximity verification backends.
///
/// Implementations verify that the exchange parties are physically near each other,
/// preventing remote QR code scanning attacks.
pub trait ProximityVerifier: Send + Sync {
    /// Emits a proximity challenge (e.g., ultrasonic audio signal).
    ///
    /// The challenge bytes should be derived from the QR code's audio_challenge field.
    fn emit_challenge(&self, challenge: &[u8; 16]) -> Result<(), ProximityError>;

    /// Listens for a proximity response.
    ///
    /// Returns the response data if received within the timeout.
    fn listen_for_response(&self, timeout: Duration) -> Result<Vec<u8>, ProximityError>;

    /// Verifies that a received response matches the expected challenge.
    fn verify_response(&self, challenge: &[u8; 16], response: &[u8]) -> bool;

    /// Performs a complete proximity verification cycle.
    ///
    /// Default implementation emits challenge, listens for response, and verifies.
    fn verify_proximity(&self, challenge: &[u8; 16], timeout: Duration) -> Result<(), ProximityError> {
        self.emit_challenge(challenge)?;
        let response = self.listen_for_response(timeout)?;
        if self.verify_response(challenge, &response) {
            Ok(())
        } else {
            Err(ProximityError::InvalidResponse)
        }
    }
}

/// Mock proximity verifier for testing.
///
/// Can be configured to always succeed, always fail, or simulate timeouts.
pub struct MockProximityVerifier {
    /// Whether verification should succeed
    pub should_succeed: bool,
    /// Whether to simulate a timeout
    pub simulate_timeout: bool,
    /// Recorded challenges (for test assertions)
    challenges: std::sync::Mutex<Vec<[u8; 16]>>,
}

impl MockProximityVerifier {
    /// Creates a new mock verifier that succeeds.
    pub fn success() -> Self {
        MockProximityVerifier {
            should_succeed: true,
            simulate_timeout: false,
            challenges: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Creates a new mock verifier that fails.
    pub fn failure() -> Self {
        MockProximityVerifier {
            should_succeed: false,
            simulate_timeout: false,
            challenges: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Creates a new mock verifier that times out.
    pub fn timeout() -> Self {
        MockProximityVerifier {
            should_succeed: false,
            simulate_timeout: true,
            challenges: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Returns the challenges that were emitted (for test assertions).
    pub fn emitted_challenges(&self) -> Vec<[u8; 16]> {
        self.challenges.lock().unwrap().clone()
    }
}

impl ProximityVerifier for MockProximityVerifier {
    fn emit_challenge(&self, challenge: &[u8; 16]) -> Result<(), ProximityError> {
        self.challenges.lock().unwrap().push(*challenge);
        Ok(())
    }

    fn listen_for_response(&self, _timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        if self.simulate_timeout {
            return Err(ProximityError::Timeout);
        }

        if self.should_succeed {
            // Return a valid response (echo the challenge with a marker)
            let mut response = Vec::with_capacity(17);
            response.push(0x01); // Success marker
            if let Some(challenge) = self.challenges.lock().unwrap().last() {
                response.extend_from_slice(challenge);
            }
            Ok(response)
        } else {
            Err(ProximityError::NoResponse)
        }
    }

    fn verify_response(&self, challenge: &[u8; 16], response: &[u8]) -> bool {
        if response.len() != 17 {
            return false;
        }
        if response[0] != 0x01 {
            return false;
        }
        &response[1..17] == challenge
    }
}

/// Manual confirmation verifier for devices without proximity hardware.
///
/// Requires users to manually confirm they are near each other (e.g., by
/// comparing fingerprints displayed on both screens).
pub struct ManualConfirmationVerifier {
    /// Whether the user confirmed proximity
    confirmed: std::sync::Mutex<bool>,
}

impl ManualConfirmationVerifier {
    /// Creates a new manual confirmation verifier.
    pub fn new() -> Self {
        ManualConfirmationVerifier {
            confirmed: std::sync::Mutex::new(false),
        }
    }

    /// Call this when the user confirms proximity.
    pub fn confirm(&self) {
        *self.confirmed.lock().unwrap() = true;
    }

    /// Check if the user has confirmed.
    pub fn is_confirmed(&self) -> bool {
        *self.confirmed.lock().unwrap()
    }
}

impl Default for ManualConfirmationVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl ProximityVerifier for ManualConfirmationVerifier {
    fn emit_challenge(&self, _challenge: &[u8; 16]) -> Result<(), ProximityError> {
        // No-op for manual verification
        Ok(())
    }

    fn listen_for_response(&self, _timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        if self.is_confirmed() {
            Ok(vec![0x01]) // Success marker
        } else {
            Err(ProximityError::NoResponse)
        }
    }

    fn verify_response(&self, _challenge: &[u8; 16], response: &[u8]) -> bool {
        // Manual verification just checks the confirmation flag
        !response.is_empty() && response[0] == 0x01
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_proximity_success() {
        let verifier = MockProximityVerifier::success();
        let challenge = [0u8; 16];

        let result = verifier.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_proximity_failure() {
        let verifier = MockProximityVerifier::failure();
        let challenge = [0u8; 16];

        let result = verifier.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_proximity_timeout() {
        let verifier = MockProximityVerifier::timeout();
        let challenge = [0u8; 16];

        let result = verifier.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(matches!(result, Err(ProximityError::Timeout)));
    }

    #[test]
    fn test_mock_records_challenges() {
        let verifier = MockProximityVerifier::success();
        let challenge1 = [1u8; 16];
        let challenge2 = [2u8; 16];

        verifier.emit_challenge(&challenge1).unwrap();
        verifier.emit_challenge(&challenge2).unwrap();

        let emitted = verifier.emitted_challenges();
        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[0], challenge1);
        assert_eq!(emitted[1], challenge2);
    }

    #[test]
    fn test_manual_confirmation() {
        let verifier = ManualConfirmationVerifier::new();
        let challenge = [0u8; 16];

        // Before confirmation, should fail
        assert!(!verifier.is_confirmed());

        // After confirmation, should succeed
        verifier.confirm();
        assert!(verifier.is_confirmed());

        let result = verifier.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(result.is_ok());
    }
}
