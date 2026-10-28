// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A configured relay must be reached over TLS: ADR-037's addendum allows
//! no plaintext relay connection. `set_relay` accepted any non-empty URL,
//! so `http://relay.example.com` was saved and used (private#883). Plain
//! http stays allowed to a loopback host only, where local relays and the
//! e2e suite run (the relay itself serves plaintext only on loopback).

use vauchi_core::{SymmetricKey, Vauchi, VauchiConfig};

const ANCHOR: [u8; 32] = [0x5a; 32];
const DEFAULT_RELAY: &str = "https://relay.vauchi.app";

const NON_TLS_URLS: &[&str] = &[
    "http://relay.example.com",
    "ws://relay.example.com",
    "wss://relay.example.com",
    "relay.example.com",
    "HTTP://relay.example.com",
    "http://relay.example.com:443",
    "http://192.168.1.10:8080",
    "http://10.0.0.1",
    "http://127.0.0.1.relay.example.com",
    "http://localhost.example.com",
    "ftp://relay.example.com",
    "file:///etc/passwd",
    "https://user:pass@relay.example.com",
    "https://relay.example.com/#frag",
    "https://relay.example.com\0.attacker",
];

// @scenario: relay_transport_security :: Non-https relay URLs are rejected at the parse boundary
#[test]
fn set_relay_refuses_every_url_that_would_skip_tls() {
    let mut vauchi = Vauchi::in_memory().unwrap();

    for url in NON_TLS_URLS {
        let result = vauchi.set_relay(url, Some(ANCHOR));

        assert!(result.is_err(), "{url:?} must be refused, got {result:?}");
        assert_eq!(
            vauchi.config().relay.server_url,
            DEFAULT_RELAY,
            "a refused {url:?} must not reach the config"
        );
    }
}

// @scenario: relay_transport_security :: No plaintext fallback exists
#[test]
fn a_refused_relay_url_is_not_persisted_for_the_next_launch() {
    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");

    {
        let config =
            VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key.clone());
        let mut vauchi = Vauchi::new(config).unwrap();
        vauchi
            .set_relay("http://relay.example.com", Some(ANCHOR))
            .unwrap_err();
    }

    let config = VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key);
    let vauchi = Vauchi::new(config).unwrap();
    assert_eq!(vauchi.config().relay.server_url, DEFAULT_RELAY);
    assert_eq!(
        vauchi.config().relay.ohttp_trust_anchor(),
        Vauchi::in_memory()
            .unwrap()
            .config()
            .relay
            .ohttp_trust_anchor(),
        "the refused relay's anchor must not replace the default one"
    );
}

// @scenario: relay_transport_security :: Relay URLs must use https
#[test]
fn set_relay_accepts_https_and_plaintext_loopback() {
    let accepted = [
        "https://relay.example.com",
        "https://relay.example.com:8443/v2",
        "  https://relay.example.com  ",
        "https://192.168.1.10",
        "http://127.0.0.1:8080",
        "http://localhost:8080",
        "http://[::1]:8080",
    ];
    let mut vauchi = Vauchi::in_memory().unwrap();

    for url in accepted {
        vauchi
            .set_relay(url, Some(ANCHOR))
            .unwrap_or_else(|error| panic!("{url:?} must be accepted: {error}"));

        assert_eq!(vauchi.config().relay.server_url, url.trim());
    }
}
