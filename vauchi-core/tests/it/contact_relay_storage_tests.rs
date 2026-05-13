// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact relay field persistence (Phase 1F / T2.5).
//!
//! Validates that relay_url and relay_noise_pubkey survive save/load
//! roundtrips through encrypted storage.

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

fn make_contact(name: &str) -> Contact {
    let public_key = [name.as_bytes()[0]; 32];
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key, 0)
}

fn open_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

// ── Save/load roundtrip ──────────────────────────────────────────

// @internal
#[test]
fn contact_relay_url_survives_storage_roundtrip() {
    let storage = open_storage();

    let mut contact = make_contact("Alice");
    contact.set_relay_url(Some("https://alice-relay.example.com".to_string()));
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert_eq!(
        loaded.relay_url().unwrap(),
        "https://alice-relay.example.com",
        "Relay URL must survive save/load roundtrip"
    );
}

// @internal
#[test]
fn contact_relay_noise_pubkey_survives_storage_roundtrip() {
    let storage = open_storage();

    let pubkey = [42u8; 32];
    let mut contact = make_contact("Bob");
    contact.set_relay_noise_pubkey(Some(pubkey));
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert_eq!(
        loaded.relay_noise_pubkey().unwrap(),
        &pubkey,
        "Relay Noise pubkey must survive save/load roundtrip"
    );
}

// @internal
#[test]
fn contact_with_both_relay_fields_roundtrips() {
    let storage = open_storage();

    let mut contact = make_contact("Carol");
    contact.set_relay_url(Some("https://carol-relay.example.com".to_string()));
    contact.set_relay_noise_pubkey(Some([99u8; 32]));
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert_eq!(
        loaded.relay_url().unwrap(),
        "https://carol-relay.example.com"
    );
    assert_eq!(loaded.relay_noise_pubkey().unwrap(), &[99u8; 32]);
}

// @internal
#[test]
fn contact_without_relay_fields_loads_as_none() {
    let storage = open_storage();

    let contact = make_contact("Dave");
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert!(loaded.relay_url().is_none());
    assert!(loaded.relay_noise_pubkey().is_none());
}

// ── Existing contacts preserved after migration ──────────────────

// @internal
#[test]
fn existing_contact_without_relay_loads_after_migration() {
    // This tests that contacts saved before the relay migration
    // still load correctly (NULL relay columns → None fields)
    let storage = open_storage();

    // Save without relay fields
    let contact = make_contact("Legacy");
    storage.save_contact(&contact).unwrap();

    // Load — should have None for relay fields
    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert!(loaded.relay_url().is_none());
    assert!(loaded.relay_noise_pubkey().is_none());
    assert_eq!(loaded.display_name(), "Legacy");
}

// ── list_contacts includes relay fields ──────────────────────────

// @internal
#[test]
fn list_contacts_includes_relay_metadata() {
    let storage = open_storage();

    let mut alice = make_contact("Alice");
    alice.set_relay_url(Some("https://alice.relay".to_string()));
    storage.save_contact(&alice).unwrap();

    let mut bob = make_contact("Bob");
    bob.set_relay_url(Some("https://bob.relay".to_string()));
    bob.set_relay_noise_pubkey(Some([11u8; 32]));
    storage.save_contact(&bob).unwrap();

    let contacts = storage.list_contacts().unwrap();
    assert_eq!(contacts.len(), 2);

    let alice_loaded = contacts
        .iter()
        .find(|c| c.display_name() == "Alice")
        .unwrap();
    assert_eq!(alice_loaded.relay_url().unwrap(), "https://alice.relay");
    assert!(alice_loaded.relay_noise_pubkey().is_none());

    let bob_loaded = contacts.iter().find(|c| c.display_name() == "Bob").unwrap();
    assert_eq!(bob_loaded.relay_url().unwrap(), "https://bob.relay");
    assert_eq!(bob_loaded.relay_noise_pubkey().unwrap(), &[11u8; 32]);
}

// ── Update relay fields ──────────────────────────────────────────

// @internal
#[test]
fn update_contact_relay_url_persists() {
    let storage = open_storage();

    let mut contact = make_contact("Eve");
    contact.set_relay_url(Some("https://old-relay.example.com".to_string()));
    storage.save_contact(&contact).unwrap();

    // Update relay URL
    contact.set_relay_url(Some("https://new-relay.example.com".to_string()));
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert_eq!(loaded.relay_url().unwrap(), "https://new-relay.example.com");
}

// @internal
#[test]
fn clear_contact_relay_url_persists() {
    let storage = open_storage();

    let mut contact = make_contact("Frank");
    contact.set_relay_url(Some("https://frank.relay".to_string()));
    storage.save_contact(&contact).unwrap();

    // Clear relay URL
    contact.set_relay_url(None);
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(contact.id()).unwrap().unwrap();
    assert!(loaded.relay_url().is_none());
}
