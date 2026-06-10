// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for QR exchange v3 with relay metadata (Phase 1A).
//!
//! Validates the extended QR binary format that includes relay URL
//! for per-contact relay routing.

use vauchi_core::exchange::{ExchangeQR, X3DHKeyPair};
use vauchi_core::identity::Identity;

// ── Roundtrip without relay metadata ───────────────────────────────

// @internal
#[test]
fn v3_roundtrip_no_relay_metadata() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();
    let qr = ExchangeQR::generate(
        &identity,
        &ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

    let data = qr.to_data_string();
    let parsed = ExchangeQR::from_data_string(&data).unwrap();

    assert_eq!(parsed.display_name(), "Alice");
    assert_eq!(parsed.public_key(), identity.signing_public_key());
    assert_eq!(parsed.exchange_key(), ephemeral.public_key());
    assert!(parsed.relay_url().is_none());
    assert!(parsed.verify_signature());
}

// ── Roundtrip with relay metadata ──────────────────────────────────

// @internal
#[test]
fn v3_roundtrip_with_relay_url_and_noise_pubkey() {
    let identity = Identity::create("Bob", 0);
    let ephemeral = X3DHKeyPair::generate();
    let relay_url = "https://relay.bobs-server.com";

    let qr =
        ExchangeQR::generate_with_relay(&identity, &ephemeral, Some(relay_url.to_string()), 0u64);

    let data = qr.to_data_string();
    let parsed = ExchangeQR::from_data_string(&data).unwrap();

    assert_eq!(parsed.display_name(), "Bob");
    assert_eq!(parsed.relay_url().unwrap(), relay_url);
    assert!(parsed.verify_signature());
}

// @internal
#[test]
fn v3_relay_url_without_noise_pubkey_now_accepted() {
    // Decision A (ADR-037): the client no longer pins a relay Noise
    // pubkey, so a relay URL with no pubkey is the normal case and must
    // parse. The gateway pins relay TLS (SPKI); operator separation
    // provides the privacy property the TOFU pin used to.
    let identity = Identity::create("Carol", 0);
    let ephemeral = X3DHKeyPair::generate();

    let qr = ExchangeQR::generate_with_relay(
        &identity,
        &ephemeral,
        Some("https://relay.example.com".to_string()),
        0u64,
    );

    let data = qr.to_data_string();
    let parsed = ExchangeQR::from_data_string(&data).expect("relay-url-only QR must parse");
    assert_eq!(parsed.relay_url().unwrap(), "https://relay.example.com");
    assert!(parsed.verify_signature());
}

// @internal
#[test]
fn v3_roundtrip_unicode_name_with_relay() {
    let identity = Identity::create("Müller 日本語", 0);
    let ephemeral = X3DHKeyPair::generate();

    let qr = ExchangeQR::generate_with_relay(
        &identity,
        &ephemeral,
        Some("https://relay.example.com".to_string()),
        0u64,
    );

    let data = qr.to_data_string();
    let parsed = ExchangeQR::from_data_string(&data).unwrap();

    assert_eq!(parsed.display_name(), "Müller 日本語");
    assert_eq!(parsed.relay_url().unwrap(), "https://relay.example.com");
    assert!(parsed.verify_signature());
}

// ── Signature integrity ────────────────────────────────────────────

// @internal
#[test]
fn v3_signature_covers_relay_fields() {
    let identity = Identity::create("Dave", 0);
    let ephemeral = X3DHKeyPair::generate();

    let qr = ExchangeQR::generate_with_relay(
        &identity,
        &ephemeral,
        Some("https://relay.example.com".to_string()),
        0u64,
    );

    // Signature must be valid
    assert!(qr.verify_signature());
}

// ── Adversarial relay URLs (CC-14) ─────────────────────────────────

// @internal
#[test]
fn v3_empty_relay_url_rejected_on_parse() {
    let identity = Identity::create("Eve", 0);
    let ephemeral = X3DHKeyPair::generate();

    let qr = ExchangeQR::generate_with_relay(&identity, &ephemeral, Some(String::new()), 0u64);

    let data = qr.to_data_string();
    let result = ExchangeQR::from_data_string(&data);

    // Empty relay URL is rejected by SSRF validation
    assert!(
        result.is_err(),
        "Empty relay URL must be rejected during QR parsing"
    );
}

// @internal
#[test]
fn v3_private_host_relay_url_rejected_on_parse() {
    let identity = Identity::create("Mallory", 0);
    let ephemeral = X3DHKeyPair::generate();

    let qr = ExchangeQR::generate_with_relay(
        &identity,
        &ephemeral,
        Some("https://127.0.0.1/evil".to_string()),
        0u64,
    );

    let data = qr.to_data_string();
    let result = ExchangeQR::from_data_string(&data);

    // Private/loopback relay URLs must be rejected (SSRF prevention)
    assert!(
        result.is_err(),
        "Private host relay URL must be rejected during QR parsing"
    );
}

// @internal
#[test]
fn v3_insecure_scheme_relay_url_rejected_on_parse() {
    let identity = Identity::create("Oscar", 0);
    let ephemeral = X3DHKeyPair::generate();

    let qr = ExchangeQR::generate_with_relay(
        &identity,
        &ephemeral,
        Some("http://relay.evil.com".to_string()),
        0u64,
    );

    let data = qr.to_data_string();
    let result = ExchangeQR::from_data_string(&data);

    // Insecure http:// scheme must be rejected
    assert!(
        result.is_err(),
        "Insecure http:// relay URL must be rejected during QR parsing"
    );
}

// @internal
#[test]
fn v3_roundtrip_long_relay_url() {
    let identity = Identity::create("Frank", 0);
    let ephemeral = X3DHKeyPair::generate();
    let long_url = format!("https://{}.example.com", "a".repeat(200));

    let qr = ExchangeQR::generate_with_relay(&identity, &ephemeral, Some(long_url.clone()), 0u64);

    let data = qr.to_data_string();
    let parsed = ExchangeQR::from_data_string(&data).unwrap();

    assert_eq!(parsed.relay_url().unwrap(), long_url);
    assert!(parsed.verify_signature());
}

// ── QR image still generates ───────────────────────────────────────

// @internal
#[test]
fn v3_qr_image_generation_with_relay() {
    let identity = Identity::create("Grace", 0);
    let ephemeral = X3DHKeyPair::generate();

    let qr = ExchangeQR::generate_with_relay(
        &identity,
        &ephemeral,
        Some("https://relay.vauchi.app".to_string()),
        0u64,
    );

    let image = qr.to_qr_image_string();
    assert!(!image.is_empty(), "QR image should be generated");
    assert!(image.contains('█'), "QR image should contain dark cells");
}
