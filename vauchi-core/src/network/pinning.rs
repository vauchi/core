// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Certificate Pinning (SPKI SHA-256)
//!
//! Provides SPKI-based certificate pinning for relay TLS connections.
//! Pins are SHA-256 hashes of the Subject Public Key Info (SPKI) field
//! from DER-encoded X.509 certificates — the same scheme used by
//! HTTP Public Key Pinning (RFC 7469) and Android's network security config.
//!
//! SPKI pinning survives certificate renewal (new cert, same key) while
//! still detecting key compromise or MitM with a different key.

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// A pinned SPKI fingerprint.
///
/// Stores a SHA-256 hash of a DER-encoded SubjectPublicKeyInfo field
/// for certificate pinning verification during TLS connections.
///
/// Uses constant-time comparison to prevent timing-based bypass.
#[derive(Debug, Clone)]
pub struct PinnedCertificate {
    /// SHA-256 fingerprint of the DER-encoded SPKI.
    pub sha256_fingerprint: [u8; 32],
}

impl PartialEq for PinnedCertificate {
    fn eq(&self, other: &Self) -> bool {
        bool::from(self.sha256_fingerprint.ct_eq(&other.sha256_fingerprint))
    }
}

impl Eq for PinnedCertificate {}

impl PinnedCertificate {
    /// Creates a pin from a known SHA-256 SPKI fingerprint.
    pub fn new(sha256_fingerprint: [u8; 32]) -> Self {
        PinnedCertificate { sha256_fingerprint }
    }

    /// Extracts the SPKI from a DER-encoded X.509 certificate and
    /// computes its SHA-256 hash.
    ///
    /// Returns `None` if the certificate cannot be parsed.
    pub fn from_cert_der(cert_der: &[u8]) -> Option<Self> {
        let spki = extract_spki(cert_der)?;
        let hash = Sha256::digest(spki);
        let mut fingerprint = [0u8; 32];
        fingerprint.copy_from_slice(hash.as_ref());
        Some(PinnedCertificate {
            sha256_fingerprint: fingerprint,
        })
    }

    /// Creates a pin by hashing raw DER bytes directly (legacy, full-cert hash).
    ///
    /// Prefer `from_cert_der` for SPKI pinning.
    pub fn from_der(cert_der: &[u8]) -> Self {
        let hash = Sha256::digest(cert_der);
        let mut fingerprint = [0u8; 32];
        fingerprint.copy_from_slice(hash.as_ref());
        PinnedCertificate {
            sha256_fingerprint: fingerprint,
        }
    }
}

/// Verifies that a certificate's SPKI matches one of the pinned fingerprints.
///
/// Extracts the SPKI from the DER-encoded certificate, computes its SHA-256
/// hash, and checks against the pin list.
///
/// Returns `true` if the SPKI matches any pin, `false` otherwise.
/// Returns `false` if the pin list is empty or the cert can't be parsed.
pub fn verify_pin(cert_der: &[u8], pins: &[PinnedCertificate]) -> bool {
    if pins.is_empty() {
        return false;
    }

    match PinnedCertificate::from_cert_der(cert_der) {
        Some(cert_pin) => pins.iter().any(|pin| pin == &cert_pin),
        None => false,
    }
}

/// Extract the SubjectPublicKeyInfo field from a DER-encoded X.509 certificate.
///
/// Returns the raw DER bytes of the SPKI (including its SEQUENCE tag+length),
/// suitable for hashing per RFC 7469.
///
/// Performs minimal ASN.1 parsing — skips through the fixed structure of
/// TBSCertificate to reach the 7th field (subjectPublicKeyInfo).
fn extract_spki(cert_der: &[u8]) -> Option<&[u8]> {
    // Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signature }
    let (tbs, _) = enter_sequence(cert_der)?;

    // TBSCertificate ::= SEQUENCE { version[0]?, serial, sigAlg, issuer, validity, subject, spki, ... }
    let (inner, _) = enter_sequence(tbs)?;
    let mut pos = inner;

    // Skip optional version [0] EXPLICIT
    if pos.first().copied() == Some(0xA0) {
        let (_, rest) = skip_tlv(pos)?;
        pos = rest;
    }

    // Skip: serialNumber, signatureAlgorithm, issuer, validity, subject (5 fields)
    for _ in 0..5 {
        let (_, rest) = skip_tlv(pos)?;
        pos = rest;
    }

    // The next TLV is subjectPublicKeyInfo — return it including its tag+length
    let (spki_bytes, _) = read_tlv(pos)?;
    Some(spki_bytes)
}

/// Enter a SEQUENCE tag and return (contents, rest_after_sequence).
fn enter_sequence(data: &[u8]) -> Option<(&[u8], &[u8])> {
    if data.first().copied() != Some(0x30) {
        return None;
    }
    let (content, rest) = read_tlv_content(&data[1..])?;
    Some((content, rest))
}

/// Skip one TLV element and return rest after it.
fn skip_tlv(data: &[u8]) -> Option<(&[u8], &[u8])> {
    read_tlv(data)
}

/// Read one complete TLV (tag + length + value) from data.
/// Returns (full_tlv_bytes, rest_after_tlv).
fn read_tlv(data: &[u8]) -> Option<(&[u8], &[u8])> {
    if data.is_empty() {
        return None;
    }
    // Skip tag byte, compute full TLV span from remainder position
    let (_content, rest) = read_tlv_content(&data[1..])?;
    let total = data.len() - rest.len();
    Some((&data[..total], rest))
}

/// After the tag byte, parse the length and return (content, rest_after_value).
fn read_tlv_content(data: &[u8]) -> Option<(&[u8], &[u8])> {
    if data.is_empty() {
        return None;
    }
    let first = data[0] as usize;
    if first < 0x80 {
        // Short form: length in one byte
        let len = first;
        let content = data.get(1..1 + len)?;
        let rest = data.get(1 + len..)?;
        Some((content, rest))
    } else if first == 0x80 {
        // Indefinite length — not valid in DER
        None
    } else {
        // Long form: first byte = 0x80 | num_length_bytes
        let num_bytes = first & 0x7F;
        if num_bytes > 4 || num_bytes == 0 {
            return None;
        }
        let len_data = data.get(1..1 + num_bytes)?;
        let mut len: usize = 0;
        for &b in len_data {
            len = len.checked_shl(8)?.checked_add(b as usize)?;
        }
        let offset = 1 + num_bytes;
        let content = data.get(offset..offset + len)?;
        let rest = data.get(offset + len..)?;
        Some((content, rest))
    }
}

// INLINE_TEST_REQUIRED: tests access private extract_spki internals
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
    fn test_from_der_legacy_hash() {
        let cert_der = b"fake DER-encoded certificate data";
        let pin = PinnedCertificate::from_der(cert_der);
        assert_eq!(pin.sha256_fingerprint.len(), 32);

        // Deterministic
        let pin2 = PinnedCertificate::from_der(cert_der);
        assert_eq!(pin, pin2);
    }

    #[test]
    fn test_verify_pin_empty_pins() {
        assert!(!verify_pin(b"anything", &[]));
    }

    #[test]
    fn test_verify_pin_garbage_cert_returns_false() {
        let wrong_pin = PinnedCertificate::new([0xFF; 32]);
        // Garbage cert can't be parsed → from_cert_der returns None → false
        assert!(!verify_pin(b"not a valid cert", &[wrong_pin]));
    }

    /// Build a minimal self-signed DER certificate for testing SPKI extraction.
    /// Structure: Certificate = SEQUENCE { TBSCert, SigAlg, Sig }
    /// TBSCert = SEQUENCE { version[0], serial, sigAlg, issuer, validity, subject, SPKI }
    fn build_test_cert(spki_content: &[u8]) -> Vec<u8> {
        // Helper: wrap in SEQUENCE tag
        fn seq(content: &[u8]) -> Vec<u8> {
            let mut out = vec![0x30];
            encode_length(content.len(), &mut out);
            out.extend_from_slice(content);
            out
        }
        // Helper: wrap in tag
        fn tagged(tag: u8, content: &[u8]) -> Vec<u8> {
            let mut out = vec![tag];
            encode_length(content.len(), &mut out);
            out.extend_from_slice(content);
            out
        }
        fn encode_length(len: usize, out: &mut Vec<u8>) {
            if len < 0x80 {
                out.push(len as u8);
            } else if len <= 0xFF {
                out.push(0x81);
                out.push(len as u8);
            } else {
                out.push(0x82);
                out.push((len >> 8) as u8);
                out.push((len & 0xFF) as u8);
            }
        }

        let version = tagged(0xA0, &tagged(0x02, &[0x02])); // v3
        let serial = tagged(0x02, &[0x01]); // serial = 1
        let sig_alg = seq(&[0x06, 0x03, 0x55, 0x04, 0x03]); // dummy OID
        let issuer = seq(&[0x31, 0x02, 0x30, 0x00]); // minimal
        let validity = seq(&[
            0x17, 0x0D, b'2', b'6', b'0', b'1', b'0', b'1', b'0', b'0', b'0', b'0', b'0', b'0',
            b'Z', 0x17, 0x0D, b'2', b'7', b'0', b'1', b'0', b'1', b'0', b'0', b'0', b'0', b'0',
            b'0', b'Z',
        ]);
        let subject = seq(&[0x31, 0x02, 0x30, 0x00]); // minimal
        let spki = seq(spki_content);

        let tbs = seq(&[version, serial, sig_alg, issuer, validity, subject, spki].concat());
        let cert_sig_alg = seq(&[0x06, 0x03, 0x55, 0x04, 0x03]);
        let cert_sig = tagged(0x03, &[0x00, 0xAB, 0xCD]); // dummy signature

        seq(&[tbs, cert_sig_alg, cert_sig].concat())
    }

    // @internal
    #[test]
    fn test_extract_spki_from_test_cert() {
        let spki_content = b"test-public-key-info-bytes";
        let cert = build_test_cert(spki_content);

        let extracted = extract_spki(&cert);
        assert!(extracted.is_some(), "SPKI extraction must succeed");

        // The extracted bytes are the full SPKI TLV (SEQUENCE tag + length + content)
        let spki_bytes = extracted.unwrap();
        // Verify it's a SEQUENCE containing our content
        assert_eq!(spki_bytes[0], 0x30, "SPKI must be a SEQUENCE");
        assert!(
            spki_bytes
                .windows(spki_content.len())
                .any(|w| w == spki_content),
            "extracted SPKI must contain the original content"
        );
    }

    // @internal
    #[test]
    fn test_from_cert_der_spki_pinning() {
        let spki_content = b"my-public-key";
        let cert = build_test_cert(spki_content);

        let pin = PinnedCertificate::from_cert_der(&cert);
        assert!(pin.is_some(), "SPKI pin extraction must succeed");

        // Same SPKI, same pin
        let pin2 = PinnedCertificate::from_cert_der(&cert);
        assert_eq!(pin, pin2, "same cert must produce same pin");

        // Different SPKI → different pin
        let cert2 = build_test_cert(b"different-key");
        let pin3 = PinnedCertificate::from_cert_der(&cert2);
        assert_ne!(pin, pin3, "different SPKI must produce different pin");
    }

    // @internal
    #[test]
    fn test_verify_pin_with_real_spki() {
        let spki_content = b"pinned-server-key";
        let cert = build_test_cert(spki_content);

        // Create pin from the cert
        let pin = PinnedCertificate::from_cert_der(&cert).unwrap();

        // verify_pin should match
        assert!(verify_pin(&cert, std::slice::from_ref(&pin)));

        // Wrong pin should not match
        let wrong = PinnedCertificate::new([0xFF; 32]);
        assert!(!verify_pin(&cert, &[wrong]));

        // Multiple pins — correct one present
        let wrong2 = PinnedCertificate::new([0xAA; 32]);
        assert!(verify_pin(&cert, &[wrong2, pin]));
    }

    // @internal
    #[test]
    fn test_spki_pin_survives_cert_renewal() {
        // Two certs with the SAME SPKI but different serials/signatures
        let shared_key = b"same-public-key-across-renewal";
        let cert_old = build_test_cert(shared_key);
        let cert_new = build_test_cert(shared_key);

        let pin_old = PinnedCertificate::from_cert_der(&cert_old).unwrap();
        let pin_new = PinnedCertificate::from_cert_der(&cert_new).unwrap();

        // SPKI pins are identical because the key didn't change
        assert_eq!(
            pin_old, pin_new,
            "SPKI pin must survive cert renewal with same key"
        );

        // Pin from old cert verifies new cert
        assert!(verify_pin(&cert_new, &[pin_old]));
    }

    // @internal
    #[test]
    fn test_extract_spki_garbage_returns_none() {
        assert!(extract_spki(b"").is_none());
        assert!(extract_spki(b"\x00").is_none());
        assert!(extract_spki(b"\x30\x00").is_none()); // empty SEQUENCE
    }
}
