// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Proximity Verification
//!
//! Trait-based proximity verification to prevent remote QR code scanning attacks.
//! Implementations can use ultrasonic audio, BLE, or other mechanisms.

use std::time::Duration;
use subtle::ConstantTimeEq;
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

    /// Returns the inherent confidence level of this verifier type.
    ///
    /// This reflects the verifier's capability, not the session result:
    /// - Ultrasonic/hardware verifiers -> High
    /// - Manual confirmation -> Medium
    /// - No-op/mock verifiers -> varies
    fn confidence_level(&self) -> super::ProximityConfidence;

    /// Performs a complete proximity verification cycle.
    ///
    /// Default implementation emits challenge, listens for response, and verifies.
    fn verify_proximity(
        &self,
        challenge: &[u8; 16],
        timeout: Duration,
    ) -> Result<(), ProximityError> {
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
        self.challenges.lock().expect("mutex poisoned").clone()
    }
}

impl ProximityVerifier for MockProximityVerifier {
    fn confidence_level(&self) -> super::ProximityConfidence {
        // Mock simulates hardware-level proximity verification
        super::ProximityConfidence::High
    }

    fn emit_challenge(&self, challenge: &[u8; 16]) -> Result<(), ProximityError> {
        self.challenges
            .lock()
            .expect("mutex poisoned")
            .push(*challenge);
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
            if let Some(challenge) = self.challenges.lock().expect("mutex poisoned").last() {
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
        response[1..17].ct_eq(challenge).into()
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

    /// Creates a verifier that is already confirmed (for testing only).
    ///
    /// Gated behind `cfg(test)` or `feature = "testing"` to prevent production
    /// code from trivially bypassing proximity verification.
    #[cfg(any(test, feature = "testing"))]
    pub fn pre_confirmed() -> Self {
        ManualConfirmationVerifier {
            confirmed: std::sync::Mutex::new(true),
        }
    }

    /// Creates a verifier with a specific initial state (for testing only).
    ///
    /// Gated behind `cfg(test)` or `feature = "testing"` to prevent production
    /// code from trivially bypassing proximity verification.
    #[cfg(any(test, feature = "testing"))]
    pub fn with_state(confirmed: bool) -> Self {
        ManualConfirmationVerifier {
            confirmed: std::sync::Mutex::new(confirmed),
        }
    }

    /// Call this when the user confirms proximity.
    pub fn confirm(&self) {
        *self.confirmed.lock().expect("mutex poisoned") = true;
    }

    /// Check if the user has confirmed.
    pub fn is_confirmed(&self) -> bool {
        *self.confirmed.lock().expect("mutex poisoned")
    }
}

impl Default for ManualConfirmationVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl ProximityVerifier for ManualConfirmationVerifier {
    fn confidence_level(&self) -> super::ProximityConfidence {
        super::ProximityConfidence::Medium
    }

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

/// Confidence level of physical proximity during contact exchange.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProximityConfidence {
    /// High confidence: verified by ultrasonic audio or NFC tap.
    High,
    /// Medium confidence: manual user confirmation.
    Medium,
    /// Low confidence: proximity check failed or timed out.
    Low,
    /// Unknown: no proximity check was performed (legacy contacts).
    #[default]
    Unknown,
}
