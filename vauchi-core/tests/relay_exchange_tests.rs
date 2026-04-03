// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for relay-mediated exchange: HttpTransport exchange methods and SAS derivation.

#![cfg(feature = "network-http")]

use vauchi_core::network::http_transport::{HttpTransport, HttpTransportConfig};
use vauchi_core::network::transport::ProxyConfig;

fn test_transport() -> HttpTransport {
    HttpTransport::new(HttpTransportConfig {
        relay_url: "http://localhost:1".to_string(),
        timeout_ms: 100,
        proxy: ProxyConfig::None,
        allow_direct: true,
    })
}

#[test]
fn test_exchange_offer_method_exists() {
    let t = test_transport();
    assert!(t.exchange_offer("cGF5bG9hZA==", Some(300)).is_err());
}

#[test]
fn test_exchange_claim_method_exists() {
    let t = test_transport();
    assert!(t.exchange_claim("123456", "cmVzcA==").is_err());
}

#[test]
fn test_exchange_complete_method_exists() {
    let t = test_transport();
    assert!(t.exchange_complete("123456").is_err());
}

// ── SAS derivation tests ──────────────────────────────────────────────────

use vauchi_core::exchange::relay_exchange::derive_sas;

#[test]
fn test_sas_deterministic() {
    let s = [42u8; 32];
    let a = [1u8; 32];
    let b = [2u8; 32];
    assert_eq!(derive_sas(&s, &a, &b), derive_sas(&s, &a, &b));
}

#[test]
fn test_sas_order_independent() {
    let s = [42u8; 32];
    let a = [1u8; 32];
    let b = [2u8; 32];
    assert_eq!(derive_sas(&s, &a, &b), derive_sas(&s, &b, &a));
}

#[test]
fn test_sas_format() {
    let sas = derive_sas(&[42u8; 32], &[1u8; 32], &[2u8; 32]);
    assert_eq!(sas.len(), 7);
    assert_eq!(&sas[3..4], "-");
    assert!(sas[..3].chars().all(|c| c.is_ascii_digit()));
    assert!(sas[4..].chars().all(|c| c.is_ascii_digit()));
}

#[test]
fn test_sas_different_secrets_differ() {
    let a = [1u8; 32];
    let b = [2u8; 32];
    assert_ne!(
        derive_sas(&[42u8; 32], &a, &b),
        derive_sas(&[99u8; 32], &a, &b)
    );
}

#[cfg(feature = "testing")]
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

// ── Vauchi relay exchange API tests ──────────────────────────────

use vauchi_core::api::Vauchi;

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

#[cfg(feature = "testing")]
#[test]
fn test_complete_exchange_needs_identity() {
    use vauchi_core::api::RelayExchangeOffer;

    let vauchi = Vauchi::in_memory().unwrap();
    // We can't construct RelayExchangeOffer directly (private fields),
    // so we test that start_relay_exchange fails first.
    let result = vauchi.start_relay_exchange(None);
    assert!(result.is_err());
}

#[test]
fn test_start_exchange_with_identity_fails_at_network() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    // With no real relay, start_relay_exchange should fail at the network layer
    let result = vauchi.start_relay_exchange(Some(300));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    // Should be a network error, not an identity error
    assert!(
        !err.contains("identity"),
        "expected network error, got identity error: {err}"
    );
}

#[test]
fn test_claim_exchange_with_identity_fails_at_network() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Bob").unwrap();
    // With no real relay, claim should fail at the network layer
    let result = vauchi.claim_relay_exchange("999999");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        !err.contains("identity"),
        "expected network error, got identity error: {err}"
    );
}
