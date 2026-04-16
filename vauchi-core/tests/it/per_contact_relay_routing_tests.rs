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

// @internal
#[test]
fn relay_for_contact_returns_contact_relay_when_set() {
    let config = MultiRelayConfig::builder()
        .primary_relay("https://home.relay")
        .build()
        .unwrap();
    let manager = MultiRelayManager::new(config);

    let contact = make_contact("Alice", Some("https://alice.relay"), None);
    let relay = manager.relay_for_contact(&contact);

    assert_eq!(relay, "https://alice.relay");
}

// @internal
#[test]
fn relay_for_contact_returns_home_relay_when_no_contact_relay() {
    let config = MultiRelayConfig::builder()
        .primary_relay("https://home.relay")
        .build()
        .unwrap();
    let manager = MultiRelayManager::new(config);

    let contact = make_contact("Bob", None, None);
    let relay = manager.relay_for_contact(&contact);

    assert_eq!(relay, "https://home.relay");
}

// @internal
#[test]
fn relay_for_contact_falls_back_when_contact_relay_unhealthy() {
    let config = MultiRelayConfig::builder()
        .primary_relay("https://home.relay")
        .build()
        .unwrap();
    let mut manager = MultiRelayManager::new(config);

    manager.mark_unhealthy("https://alice.relay");

    let contact = make_contact("Alice", Some("https://alice.relay"), None);
    let relay = manager.relay_for_contact(&contact);

    assert_eq!(
        relay, "https://home.relay",
        "Should fall back to home relay when contact's relay is unhealthy"
    );
}

// ── add_contact_relay ──────────────────────────────────────────────

// @internal
#[test]
fn add_contact_relay_tracked_for_health() {
    let config = MultiRelayConfig::builder()
        .primary_relay("https://home.relay")
        .build()
        .unwrap();
    let mut manager = MultiRelayManager::new(config);

    manager.add_contact_relay("https://alice.relay");

    assert!(manager.is_relay_healthy("https://alice.relay"));
    assert!(
        manager
            .contact_relay_urls()
            .contains(&"https://alice.relay")
    );
}

// @internal
#[test]
fn add_contact_relay_deduplication() {
    let config = MultiRelayConfig::builder()
        .primary_relay("https://home.relay")
        .build()
        .unwrap();
    let mut manager = MultiRelayManager::new(config);

    manager.add_contact_relay("https://alice.relay");
    manager.add_contact_relay("https://alice.relay");

    // Should only be tracked once
    let urls = manager.contact_relay_urls();
    assert_eq!(
        urls.iter().filter(|u| **u == "https://alice.relay").count(),
        1
    );
}

// ── all_relay_urls ─────────────────────────────────────────────────

// @internal
#[test]
fn all_relay_urls_includes_home_and_contact_relays() {
    let config = MultiRelayConfig::builder()
        .primary_relay("https://home.relay")
        .build()
        .unwrap();
    let mut manager = MultiRelayManager::new(config);
    manager.add_contact_relay("https://alice.relay");
    manager.add_contact_relay("https://bob.relay");

    let all = manager.all_relay_urls();

    assert!(all.contains(&"https://home.relay".to_string()));
    assert!(all.contains(&"https://alice.relay".to_string()));
    assert!(all.contains(&"https://bob.relay".to_string()));
}

// ── fallback and recovery ─────────────────────────────────────────

// @internal
#[test]
fn unhealthy_contact_relay_falls_back_then_recovers() {
    let config = MultiRelayConfig::builder()
        .primary_relay("https://home.relay")
        .build()
        .unwrap();
    let mut manager = MultiRelayManager::new(config);
    manager.add_contact_relay("https://alice.relay");

    let alice = make_contact("Alice", Some("https://alice.relay"), None);

    // Initially healthy → uses contact relay
    assert_eq!(manager.relay_for_contact(&alice), "https://alice.relay");

    // Mark unhealthy → falls back to home
    manager.mark_unhealthy("https://alice.relay");
    assert_eq!(
        manager.relay_for_contact(&alice),
        "https://home.relay",
        "Should fall back to home when contact relay is unhealthy"
    );

    // Recovery → uses contact relay again
    manager.mark_healthy("https://alice.relay");
    assert_eq!(
        manager.relay_for_contact(&alice),
        "https://alice.relay",
        "Should return to contact relay after recovery"
    );
}

// @internal
#[test]
fn contacts_without_relay_always_use_home_regardless_of_health() {
    let config = MultiRelayConfig::builder()
        .primary_relay("https://home.relay")
        .add_relay("https://backup.relay")
        .build()
        .unwrap();
    let manager = MultiRelayManager::new(config);

    let bob = make_contact("Bob", None, None);

    // No relay set → home relay
    let relay = manager.relay_for_contact(&bob);
    assert_eq!(relay, "https://home.relay");
}

// @internal
#[test]
fn empty_relay_url_treated_as_no_relay() {
    let config = MultiRelayConfig::builder()
        .primary_relay("https://home.relay")
        .build()
        .unwrap();
    let manager = MultiRelayManager::new(config);

    let contact = make_contact("Eve", Some(""), None);
    assert_eq!(
        manager.relay_for_contact(&contact),
        "https://home.relay",
        "Empty relay URL should fall back to home relay"
    );
}

// @internal
#[test]
fn mixed_relay_contacts_route_independently() {
    let config = MultiRelayConfig::builder()
        .primary_relay("https://home.relay")
        .build()
        .unwrap();
    let mut manager = MultiRelayManager::new(config);
    manager.add_contact_relay("https://alice.relay");
    manager.add_contact_relay("https://carol.relay");

    let alice = make_contact("Alice", Some("https://alice.relay"), None);
    let bob = make_contact("Bob", None, None);
    let carol = make_contact("Carol", Some("https://carol.relay"), None);

    // Each contact routes independently
    assert_eq!(manager.relay_for_contact(&alice), "https://alice.relay");
    assert_eq!(manager.relay_for_contact(&bob), "https://home.relay");
    assert_eq!(manager.relay_for_contact(&carol), "https://carol.relay");

    // Mark only Alice's relay unhealthy — Carol unaffected
    manager.mark_unhealthy("https://alice.relay");
    assert_eq!(manager.relay_for_contact(&alice), "https://home.relay");
    assert_eq!(manager.relay_for_contact(&carol), "https://carol.relay");
}

// ── group_by_relay ─────────────────────────────────────────────────

// @internal
#[test]
fn group_contacts_by_relay() {
    let config = MultiRelayConfig::builder()
        .primary_relay("https://home.relay")
        .build()
        .unwrap();
    let manager = MultiRelayManager::new(config);

    let contacts = vec![
        make_contact("Alice", Some("https://alice.relay"), None),
        make_contact("Bob", None, None),
        make_contact("Carol", Some("https://alice.relay"), None),
        make_contact("Dave", Some("https://dave.relay"), None),
    ];

    let groups = manager.group_contacts_by_relay(&contacts);

    // Home relay: Bob
    assert!(groups.get("https://home.relay").unwrap().contains(&"Bob"));
    // Alice's relay: Alice, Carol
    let alice_relay = groups.get("https://alice.relay").unwrap();
    assert!(alice_relay.contains(&"Alice"));
    assert!(alice_relay.contains(&"Carol"));
    // Dave's relay: Dave
    assert!(groups.get("https://dave.relay").unwrap().contains(&"Dave"));
}
