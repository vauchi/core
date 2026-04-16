// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Test verifier helpers for proximity verification tests.
//!
//! Provides verifier wrappers that capture emitted challenges for assertion
//! without requiring direct access to the session's verifier field.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use vauchi_core::exchange::{ProximityConfidence, ProximityError, ProximityVerifier};

/// A ProximityVerifier wrapper that captures emitted challenges in a shared
/// buffer, allowing tests to inspect which challenges were passed to the
/// verifier without needing access to the session's internal verifier field.
///
/// # Usage
/// ```ignore
/// let (verifier, challenges) = ChallengeCapturingVerifier::success();
/// let mut session = ExchangeSession::new_qr(identity, card, verifier);
/// // ... drive session through exchange ...
/// let emitted = challenges.lock().unwrap();
/// assert_eq!(emitted[0], expected_challenge);
/// ```
pub struct ChallengeCapturingVerifier {
    should_succeed: bool,
    captured: Arc<Mutex<Vec<[u8; 16]>>>,
}

impl ChallengeCapturingVerifier {
    /// Creates a verifier that succeeds and captures challenges.
    /// Returns the verifier and a shared handle to the captured challenges.
    pub fn success() -> (Self, Arc<Mutex<Vec<[u8; 16]>>>) {
        let captured = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                should_succeed: true,
                captured: captured.clone(),
            },
            captured,
        )
    }

    /// Creates a verifier that fails and captures challenges.
    pub fn failure() -> (Self, Arc<Mutex<Vec<[u8; 16]>>>) {
        let captured = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                should_succeed: false,
                captured: captured.clone(),
            },
            captured,
        )
    }
}

impl ProximityVerifier for ChallengeCapturingVerifier {
    fn confidence_level(&self) -> ProximityConfidence {
        ProximityConfidence::High
    }

    fn emit_challenge(&self, challenge: &[u8; 16]) -> Result<(), ProximityError> {
        self.captured
            .lock()
            .expect("mutex poisoned")
            .push(*challenge);
        Ok(())
    }

    fn listen_for_response(&self, _timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        if self.should_succeed {
            let mut response = Vec::with_capacity(17);
            response.push(0x01);
            if let Some(challenge) = self.captured.lock().expect("mutex poisoned").last() {
                response.extend_from_slice(challenge);
            }
            Ok(response)
        } else {
            Err(ProximityError::NoResponse)
        }
    }

    fn verify_response(&self, _challenge: &[u8; 16], response: &[u8]) -> bool {
        // Ignores the challenge parameter: this verifier runs in a single process
        // and cannot simulate the peer independently echoing our challenge.
        // Real verifiers (UltrasonicVerifier) do cryptographic challenge matching.
        response.len() == 17 && response[0] == 0x01 && self.should_succeed
    }
}
