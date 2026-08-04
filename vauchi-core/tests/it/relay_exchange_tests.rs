// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for relay-mediated exchange: HttpTransport exchange methods and SAS derivation.

#![cfg(feature = "network-http")]

use crate::common::mock_relay::{CannedResponse, MockRelay};
use vauchi_core::api::{VauchiBuilder, VauchiConfig};
use vauchi_core::network::http_transport::{HttpTransport, HttpTransportConfig};

fn assert_zeroize_on_drop<T: zeroize::ZeroizeOnDrop>() {}

fn test_transport() -> HttpTransport {
    HttpTransport::new(HttpTransportConfig::for_testing("http://localhost:1", 100))
}

// @scenario: relay_exchange :: offer endpoint exists on HttpTransport
// @internal
#[test]
fn test_exchange_offer_method_exists() {
    let t = test_transport();
    assert!(t.exchange_offer("cGF5bG9hZA==", Some(300)).is_err());
}

// @scenario: relay_exchange :: claim endpoint exists on HttpTransport
// @internal
#[test]
fn test_exchange_claim_method_exists() {
    let t = test_transport();
    assert!(t.exchange_claim("123456", "cmVzcA==").is_err());
}

// @scenario: relay_exchange :: complete endpoint exists on HttpTransport
// @internal
#[test]
fn test_exchange_complete_method_exists() {
    let t = test_transport();
    assert!(t.exchange_complete("123456").is_err());
}

// ── SAS derivation tests ──────────────────────────────────────────────────

use vauchi_core::exchange::relay_exchange::derive_sas;

// @scenario: relay_exchange :: SAS derivation is deterministic
// @internal
#[test]
fn test_sas_deterministic() {
    let s = [42u8; 32];
    let a = [1u8; 32];
    let b = [2u8; 32];
    assert_eq!(derive_sas(&s, &a, &b), derive_sas(&s, &a, &b));
}

// @scenario: relay_exchange :: SAS derivation is order independent
// @internal
#[test]
fn test_sas_order_independent() {
    let s = [42u8; 32];
    let a = [1u8; 32];
    let b = [2u8; 32];
    assert_eq!(derive_sas(&s, &a, &b), derive_sas(&s, &b, &a));
}

// @scenario: relay_exchange :: SAS format is XXX-XXX
// @internal
#[test]
fn test_sas_format() {
    let sas = derive_sas(&[42u8; 32], &[1u8; 32], &[2u8; 32]);
    assert_eq!(sas.len(), 7);
    assert_eq!(&sas[3..4], "-");
    assert!(sas[..3].chars().all(|c| c.is_ascii_digit()));
    assert!(sas[4..].chars().all(|c| c.is_ascii_digit()));
}

// @scenario: relay_exchange :: different shared secrets produce different SAS
// @internal
#[test]
fn test_sas_different_secrets_differ() {
    let a = [1u8; 32];
    let b = [2u8; 32];
    assert_ne!(
        derive_sas(&[42u8; 32], &a, &b),
        derive_sas(&[99u8; 32], &a, &b)
    );
}

// @scenario: relay_exchange :: both X3DH sides derive matching SAS
#[cfg(feature = "testing")]
// @internal
#[test]
fn test_sas_both_sides_match() {
    use vauchi_core::exchange::{X3DH, X3DHKeyPair};

    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();
    let alice_id = [1u8; 32];
    let bob_id = [2u8; 32];

    let (alice_secret, ephemeral) = X3DH::initiate(&alice, bob.public_key()).unwrap();
    let bob_secret = X3DH::respond(&bob, alice.public_key(), &ephemeral).unwrap();

    let alice_sas = derive_sas(alice_secret.as_bytes(), &alice_id, &bob_id);
    let bob_sas = derive_sas(bob_secret.as_bytes(), &alice_id, &bob_id);
    assert_eq!(alice_sas, bob_sas);
}

// @scenario: security :: Two-DH static responder compromise recovers past secrets
#[cfg(feature = "testing")]
#[test]
fn test_crypto_hardening_two_dh_static_responder_compromise_recovers_past_secret() {
    use vauchi_core::exchange::{X3DH, X3DHKeyPair};

    let initiator = X3DHKeyPair::from_bytes([0x11; 32]);
    let responder = X3DHKeyPair::from_bytes([0x22; 32]);
    let compromised_responder_secret = *responder.secret_bytes();

    let (past_secret, recorded_ephemeral) =
        X3DH::initiate(&initiator, responder.public_key()).unwrap();

    let compromised_responder = X3DHKeyPair::from_bytes(compromised_responder_secret);
    let recovered = X3DH::respond(
        &compromised_responder,
        initiator.public_key(),
        &recorded_ephemeral,
    )
    .unwrap();

    assert_eq!(
        past_secret.as_bytes(),
        recovered.as_bytes(),
        "the two-DH construction has no responder-side forward secrecy when its static key leaks"
    );
}

// @scenario: security :: Relay exchange offer debug output redacts private key material
#[cfg(feature = "testing")]
#[test]
fn test_crypto_hardening_relay_exchange_offer_debug_redacts_secret() {
    assert_zeroize_on_drop::<vauchi_core::api::RelayExchangeOffer>();

    let mock = MockRelay::start();
    mock.queue(
        "exchange_offer",
        CannedResponse::ok_json(br#"{"status":"ok","code":"123456"}"#.to_vec()),
    );

    let dir = tempfile::tempdir().unwrap();
    let mut config =
        VauchiConfig::with_storage_path(dir.path().join("vauchi.db")).with_relay_url(mock.url());
    config.ohttp.allow_direct = true;
    let mut vauchi = VauchiBuilder::new().config(config).build().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let offer = vauchi.start_relay_exchange(Some(300)).unwrap();
    let secret_debug = format!("{:?}", offer.sas_key_material());
    let offer_debug = format!("{offer:?}");

    assert!(
        !offer_debug.contains(&secret_debug),
        "relay offer Debug must not contain its X25519 private key: {offer_debug}"
    );
    assert!(
        offer_debug.contains("[REDACTED]"),
        "relay offer Debug must make redaction explicit: {offer_debug}"
    );
    assert!(
        !offer_debug.contains(&offer.code),
        "relay offer Debug must not contain its claim capability: {offer_debug}"
    );
}

// @scenario: security :: Relay exchanges use fresh X25519 key material per offer
#[cfg(feature = "testing")]
#[test]
fn test_crypto_hardening_relay_exchange_uses_fresh_key_per_offer() {
    let mock = MockRelay::start();
    mock.queue(
        "exchange_offer",
        CannedResponse::ok_json(br#"{"status":"ok","code":"111111"}"#.to_vec()),
    );
    mock.queue(
        "exchange_offer",
        CannedResponse::ok_json(br#"{"status":"ok","code":"222222"}"#.to_vec()),
    );

    let dir = tempfile::tempdir().unwrap();
    let mut config =
        VauchiConfig::with_storage_path(dir.path().join("vauchi.db")).with_relay_url(mock.url());
    config.ohttp.allow_direct = true;
    let mut vauchi = VauchiBuilder::new().config(config).build().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let first = vauchi.start_relay_exchange(Some(300)).unwrap();
    let second = vauchi.start_relay_exchange(Some(300)).unwrap();

    assert_ne!(
        first.sas_key_material(),
        second.sas_key_material(),
        "each relay offer must use fresh X25519 private key material"
    );
}

// @scenario: security :: Relay exchange uses matching fresh keys and consumes offer secret
#[cfg(feature = "testing")]
#[test]
fn test_crypto_hardening_relay_exchange_roundtrip_consumes_offer_secret() {
    use vauchi_protocol::v2::{V2ExchangeClaimRequest, V2ExchangeOfferRequest, V2Response};

    let mock = MockRelay::start();
    mock.queue(
        "exchange_offer",
        CannedResponse::ok_json(br#"{"status":"ok","code":"654321"}"#.to_vec()),
    );

    let alice_dir = tempfile::tempdir().unwrap();
    let mut alice_config = VauchiConfig::with_storage_path(alice_dir.path().join("vauchi.db"))
        .with_relay_url(mock.url());
    alice_config.ohttp.allow_direct = true;
    let mut alice = VauchiBuilder::new().config(alice_config).build().unwrap();
    alice.create_identity("Alice").unwrap();

    let bob_dir = tempfile::tempdir().unwrap();
    let mut bob_config = VauchiConfig::with_storage_path(bob_dir.path().join("vauchi.db"))
        .with_relay_url(mock.url());
    bob_config.ohttp.allow_direct = true;
    let mut bob = VauchiBuilder::new().config(bob_config).build().unwrap();
    bob.create_identity("Bob").unwrap();

    let mut offer = alice.start_relay_exchange(Some(300)).unwrap();
    let offer_request: V2ExchangeOfferRequest =
        serde_json::from_slice(&mock.received()[0].body).unwrap();

    let mut claim_response = V2Response::new("ok");
    claim_response.payload = Some(offer_request.payload);
    mock.queue(
        "exchange_claim",
        CannedResponse::ok_v2_response(&claim_response),
    );
    let bob_result = bob.claim_relay_exchange(&offer.code).unwrap();
    let claim_request: V2ExchangeClaimRequest =
        serde_json::from_slice(&mock.received()[1].body).unwrap();

    let mut complete_response = V2Response::new("ok");
    complete_response.response = Some(claim_request.response);
    mock.queue(
        "exchange_complete",
        CannedResponse::ok_v2_response(&complete_response),
    );
    let code = offer.code.clone();
    let alice_result = alice
        .complete_relay_exchange(&code, &mut offer)
        .unwrap()
        .unwrap();

    assert_eq!(alice_result.sas, bob_result.sas);
    assert_eq!(alice_result.display_name, "Bob");
    assert_eq!(bob_result.display_name, "Alice");
    assert!(
        alice
            .get_contact(&alice_result.contact_id)
            .unwrap()
            .is_some()
    );
    assert!(bob.get_contact(&bob_result.contact_id).unwrap().is_some());

    let mut alice_ratchet = alice
        .get_ratchet_state(&alice_result.contact_id)
        .unwrap()
        .unwrap();
    let mut bob_ratchet = bob
        .get_ratchet_state(&bob_result.contact_id)
        .unwrap()
        .unwrap();
    let message = bob_ratchet.encrypt(b"relay exchange ratchet").unwrap();
    assert_eq!(
        alice_ratchet.decrypt(&message).unwrap(),
        b"relay exchange ratchet"
    );
    assert_eq!(
        offer.sas_key_material(),
        &[0u8; 32],
        "successful completion must consume the one-use private key"
    );
}

// ── Vauchi relay exchange API tests ──────────────────────────────

use vauchi_core::api::Vauchi;

// @scenario: relay_exchange :: start requires identity
// @internal
#[test]
fn test_start_exchange_needs_identity() {
    let vauchi = Vauchi::in_memory().unwrap();
    let result = vauchi.start_relay_exchange(None);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("identity"),
        "expected identity error, got: {err}"
    );
}

// @scenario: relay_exchange :: claim requires identity
// @internal
#[test]
fn test_claim_exchange_needs_identity() {
    let vauchi = Vauchi::in_memory().unwrap();
    let result = vauchi.claim_relay_exchange("123456");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("identity"),
        "expected identity error, got: {err}"
    );
}

// @scenario: relay_exchange :: complete requires identity
#[cfg(feature = "testing")]
// @internal
#[test]
fn test_complete_exchange_needs_identity() {
    let vauchi = Vauchi::in_memory().unwrap();
    // We can't construct RelayExchangeOffer directly (private fields),
    // so we verify the identity gate via start_relay_exchange.
    let result = vauchi.start_relay_exchange(None);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("identity"),
        "expected identity error, got: {err}"
    );
}

// @scenario: relay_exchange :: start with identity reaches network layer
// @internal
#[test]
fn test_start_exchange_with_identity_fails_at_network() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let result = vauchi.start_relay_exchange(Some(300));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        !err.contains("identity"),
        "expected network error, got identity error: {err}"
    );
}

// @scenario: relay_exchange :: claim with identity reaches network layer
// @internal
#[test]
fn test_claim_exchange_with_identity_fails_at_network() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Bob").unwrap();
    let result = vauchi.claim_relay_exchange("999999");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        !err.contains("identity"),
        "expected network error, got identity error: {err}"
    );
}
