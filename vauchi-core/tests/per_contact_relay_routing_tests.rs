// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for per-contact relay routing (Phase 1C).
//!
//! Validates that MultiRelayManager tracks contact relays and
//! routes correctly per-contact.

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::{MultiRelayConfig, MultiRelayManager};

fn make_contact(name: &str, relay_url: Option<&str>, noise_pubkey: Option<[u8; 32]>) -> Contact {
    let public_key = [name.as_bytes()[0]; 32];
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    let mut contact = Contact::from_exchange(public_key, card, shared_key);
    contact.set_relay_url(relay_url.map(String::from));
    contact.set_relay_noise_pubkey(noise_pubkey);
    contact
}

// ── relay_for_contact ──────────────────────────────────────────────

#[test]
fn relay_for_contact_returns_contact_relay_when_set() {
    let config = MultiRelayConfig::builder()
        .primary_relay("wss://home.relay")
        .build()
        .unwrap();
    let manager = MultiRelayManager::new(config);

    let contact = make_contact("Alice", Some("wss://alice.relay"), None);
    let relay = manager.relay_for_contact(&contact);

    assert_eq!(relay, "wss://alice.relay");
}

#[test]
fn relay_for_contact_returns_home_relay_when_no_contact_relay() {
    let config = MultiRelayConfig::builder()
        .primary_relay("wss://home.relay")
        .build()
        .unwrap();
    let manager = MultiRelayManager::new(config);

    let contact = make_contact("Bob", None, None);
    let relay = manager.relay_for_contact(&contact);

    assert_eq!(relay, "wss://home.relay");
}

#[test]
fn relay_for_contact_falls_back_when_contact_relay_unhealthy() {
    let config = MultiRelayConfig::builder()
        .primary_relay("wss://home.relay")
        .build()
        .unwrap();
    let mut manager = MultiRelayManager::new(config);

    manager.mark_unhealthy("wss://alice.relay");

    let contact = make_contact("Alice", Some("wss://alice.relay"), None);
    let relay = manager.relay_for_contact(&contact);

    assert_eq!(
        relay, "wss://home.relay",
        "Should fall back to home relay when contact's relay is unhealthy"
    );
}

// ── add_contact_relay ──────────────────────────────────────────────

#[test]
fn add_contact_relay_tracked_for_health() {
    let config = MultiRelayConfig::builder()
        .primary_relay("wss://home.relay")
        .build()
        .unwrap();
    let mut manager = MultiRelayManager::new(config);

    manager.add_contact_relay("wss://alice.relay");

    assert!(manager.is_relay_healthy("wss://alice.relay"));
    assert!(manager.contact_relay_urls().contains(&"wss://alice.relay"));
}

#[test]
fn add_contact_relay_deduplication() {
    let config = MultiRelayConfig::builder()
        .primary_relay("wss://home.relay")
        .build()
        .unwrap();
    let mut manager = MultiRelayManager::new(config);

    manager.add_contact_relay("wss://alice.relay");
    manager.add_contact_relay("wss://alice.relay");

    // Should only be tracked once
    let urls = manager.contact_relay_urls();
    assert_eq!(
        urls.iter().filter(|u| **u == "wss://alice.relay").count(),
        1
    );
}

// ── all_relay_urls ─────────────────────────────────────────────────

#[test]
fn all_relay_urls_includes_home_and_contact_relays() {
    let config = MultiRelayConfig::builder()
        .primary_relay("wss://home.relay")
        .build()
        .unwrap();
    let mut manager = MultiRelayManager::new(config);
    manager.add_contact_relay("wss://alice.relay");
    manager.add_contact_relay("wss://bob.relay");

    let all = manager.all_relay_urls();

    assert!(all.contains(&"wss://home.relay".to_string()));
    assert!(all.contains(&"wss://alice.relay".to_string()));
    assert!(all.contains(&"wss://bob.relay".to_string()));
}

// ── group_by_relay ─────────────────────────────────────────────────

#[test]
fn group_contacts_by_relay() {
    let config = MultiRelayConfig::builder()
        .primary_relay("wss://home.relay")
        .build()
        .unwrap();
    let manager = MultiRelayManager::new(config);

    let contacts = vec![
        make_contact("Alice", Some("wss://alice.relay"), None),
        make_contact("Bob", None, None),
        make_contact("Carol", Some("wss://alice.relay"), None),
        make_contact("Dave", Some("wss://dave.relay"), None),
    ];

    let groups = manager.group_contacts_by_relay(&contacts);

    // Home relay: Bob
    assert!(groups.get("wss://home.relay").unwrap().contains(&"Bob"));
    // Alice's relay: Alice, Carol
    let alice_relay = groups.get("wss://alice.relay").unwrap();
    assert!(alice_relay.contains(&"Alice"));
    assert!(alice_relay.contains(&"Carol"));
    // Dave's relay: Dave
    assert!(groups.get("wss://dave.relay").unwrap().contains(&"Dave"));
}
