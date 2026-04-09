// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Certificate Pinning
//!
//! Provides certificate pinning support for relay connections.
//! Pins are SHA-256 fingerprints of DER-encoded certificates.
//!
//! ## Integration Status
//!
//! The pinning primitives (`PinnedCertificate`, `verify_pin`) are implemented
//! and tested. Wiring into `HttpTransport` (ureq/rustls) via a custom
//! `ServerCertVerifier` is the next step (C7 Phase 2).

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// A pinned certificate fingerprint.
///
/// Stores a SHA-256 hash of a DER-encoded certificate for
/// certificate pinning verification during TLS connections.
///
/// Uses constant-time comparison to prevent timing-based
/// certificate pinning bypass attacks.
#[derive(Debug, Clone)]
pub struct PinnedCertificate {
    /// SHA-256 fingerprint of the DER-encoded certificate.
    pub sha256_fingerprint: [u8; 32],
}

impl PartialEq for PinnedCertificate {
    fn eq(&self, other: &Self) -> bool {
        bool::from(self.sha256_fingerprint.ct_eq(&other.sha256_fingerprint))
    }
}

impl Eq for PinnedCertificate {}

impl PinnedCertificate {
    /// Creates a new pinned certificate from a known SHA-256 fingerprint.
    pub fn new(sha256_fingerprint: [u8; 32]) -> Self {
        PinnedCertificate { sha256_fingerprint }
    }

    /// Computes SHA-256 hash of DER-encoded certificate bytes and creates
    /// a pinned certificate from the result.
    pub fn from_der(cert_der: &[u8]) -> Self {
        let hash = Sha256::digest(cert_der);
        let mut fingerprint = [0u8; 32];
        fingerprint.copy_from_slice(hash.as_ref());
        PinnedCertificate {
            sha256_fingerprint: fingerprint,
        }
    }
}

/// Verifies that a certificate matches one of the pinned fingerprints.
///
/// Computes the SHA-256 hash of the given DER-encoded certificate and
/// checks it against the provided list of pinned certificates.
///
/// Returns `true` if the certificate matches any pin, `false` otherwise.
/// Returns `false` if the pin list is empty.
pub fn verify_pin(cert_der: &[u8], pins: &[PinnedCertificate]) -> bool {
    if pins.is_empty() {
        return false;
    }

    let cert_pin = PinnedCertificate::from_der(cert_der);
    pins.iter().any(|pin| pin == &cert_pin)
}

// INLINE_TEST_REQUIRED: tests access private internals
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pinned_certificate_new() {
        let fingerprint = [0xAA; 32];
        let pin = PinnedCertificate::new(fingerprint);
        assert_eq!(pin.sha256_fingerprint, fingerprint);
    }

    #[test]
    fn test_pinned_certificate_from_der() {
        let cert_der = b"fake DER-encoded certificate data";
        let pin = PinnedCertificate::from_der(cert_der);

        // Verify it produces a valid 32-byte fingerprint
        assert_eq!(pin.sha256_fingerprint.len(), 32);

        // Verify deterministic: same input produces same output
        let pin2 = PinnedCertificate::from_der(cert_der);
        assert_eq!(pin, pin2);
    }

    #[test]
    fn test_pinned_certificate_from_der_different_inputs() {
        let cert1 = b"certificate one";
        let cert2 = b"certificate two";

        let pin1 = PinnedCertificate::from_der(cert1);
        let pin2 = PinnedCertificate::from_der(cert2);

        assert_ne!(pin1, pin2);
    }

    #[test]
    fn test_verify_pin_matching() {
        let cert_der = b"test certificate data";
        let pin = PinnedCertificate::from_der(cert_der);

        assert!(verify_pin(cert_der, &[pin]));
    }

    #[test]
    fn test_verify_pin_no_match() {
        let cert_der = b"test certificate data";
        let wrong_pin = PinnedCertificate::new([0xFF; 32]);

        assert!(!verify_pin(cert_der, &[wrong_pin]));
    }

    #[test]
    fn test_verify_pin_empty_pins() {
        let cert_der = b"test certificate data";
        assert!(!verify_pin(cert_der, &[]));
    }

    #[test]
    fn test_verify_pin_multiple_pins() {
        let cert_der = b"test certificate data";
        let correct_pin = PinnedCertificate::from_der(cert_der);
        let wrong_pin = PinnedCertificate::new([0xFF; 32]);

        // Should match when the correct pin is in the list
        assert!(verify_pin(cert_der, &[wrong_pin.clone(), correct_pin]));

        // Should not match when only wrong pins are in the list
        assert!(!verify_pin(cert_der, &[wrong_pin]));
    }

    #[test]
    fn test_verify_pin_uses_sha256() {
        // Verify that from_der produces the same hash as sha2::Sha256
        let cert_der = b"verify SHA-256 consistency";
        let expected = Sha256::digest(cert_der);
        let pin = PinnedCertificate::from_der(cert_der);

        assert_eq!(pin.sha256_fingerprint.as_slice(), &expected[..]);
    }
}
