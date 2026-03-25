// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for Contact relay metadata fields (Phase 1B).
//!
//! Verifies that contacts can store relay URL and Noise NK pubkey
//! learned during exchange, for per-contact relay routing.

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

fn make_test_contact() -> Contact {
    let public_key = [1u8; 32];
    let card = ContactCard::new("Bob");
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key)
}

// ── Default state ──────────────────────────────────────────────────

#[test]
fn new_contact_has_no_relay_metadata() {
    let contact = make_test_contact();
    assert!(
        contact.relay_url().is_none(),
        "New contacts should have no relay URL"
    );
    assert!(
        contact.relay_noise_pubkey().is_none(),
        "New contacts should have no relay Noise pubkey"
    );
}

// ── Setting relay metadata ─────────────────────────────────────────

#[test]
fn set_relay_url() {
    let mut contact = make_test_contact();
    contact.set_relay_url(Some("wss://relay.bobs-server.com".to_string()));
    assert_eq!(contact.relay_url().unwrap(), "wss://relay.bobs-server.com");
}

#[test]
fn set_relay_noise_pubkey() {
    let mut contact = make_test_contact();
    let pubkey = [42u8; 32];
    contact.set_relay_noise_pubkey(Some(pubkey));
    assert_eq!(contact.relay_noise_pubkey().unwrap(), &pubkey);
}

#[test]
fn clear_relay_metadata() {
    let mut contact = make_test_contact();
    contact.set_relay_url(Some("wss://relay.example.com".to_string()));
    contact.set_relay_noise_pubkey(Some([99u8; 32]));

    contact.set_relay_url(None);
    contact.set_relay_noise_pubkey(None);

    assert!(contact.relay_url().is_none());
    assert!(contact.relay_noise_pubkey().is_none());
}

// ── Constructors preserve relay fields ─────────────────────────────

#[test]
fn from_exchange_has_no_relay_fields() {
    let contact = Contact::from_exchange(
        [2u8; 32],
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
    );
    assert!(contact.relay_url().is_none());
    assert!(contact.relay_noise_pubkey().is_none());
}

#[test]
fn from_sync_data_has_no_relay_fields() {
    let contact = Contact::from_sync_data(
        [3u8; 32],
        ContactCard::new("Charlie"),
        SymmetricKey::generate(),
        1000,
        false,
        vauchi_core::contact::VisibilityRules::new(),
    );
    assert!(contact.relay_url().is_none());
    assert!(contact.relay_noise_pubkey().is_none());
}

// ── Relay metadata survives card update ────────────────────────────

#[test]
fn relay_metadata_preserved_after_card_update() {
    let mut contact = make_test_contact();
    contact.set_relay_url(Some("wss://relay.example.com".to_string()));
    contact.set_relay_noise_pubkey(Some([77u8; 32]));

    let new_card = ContactCard::new("Bob Updated");
    contact.update_card(new_card);

    assert_eq!(contact.relay_url().unwrap(), "wss://relay.example.com");
    assert_eq!(contact.relay_noise_pubkey().unwrap(), &[77u8; 32]);
}

// ── Relay metadata survives recovery ───────────────────────────────

#[test]
fn relay_metadata_preserved_after_recovery() {
    let mut contact = make_test_contact();
    contact.set_relay_url(Some("wss://relay.example.com".to_string()));
    contact.set_relay_noise_pubkey(Some([88u8; 32]));

    contact
        .accept_recovery([5u8; 32], SymmetricKey::generate())
        .unwrap();

    // Relay metadata should persist through recovery — the contact
    // is still reachable at the same relay.
    assert_eq!(contact.relay_url().unwrap(), "wss://relay.example.com");
    assert_eq!(contact.relay_noise_pubkey().unwrap(), &[88u8; 32]);
}
