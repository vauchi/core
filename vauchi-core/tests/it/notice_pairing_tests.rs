// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Opening the two directions between one own device and one peer device
//! from the contact's shared key (vauchi/private#1375, step 2, decision
//! A1): each side derives on its own the seed the other side derives.
#![cfg(feature = "notice-core")]

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{SymmetricKey, X25519KeyPair};
use vauchi_core::identity::Identity;
use vauchi_core::notice::direction::Sender;
use vauchi_core::notice::pairing::{PeerDevice, ensure_directions, open_directions};
use vauchi_core::storage::{NoticeStateKind, Storage};

const NOW: u64 = 1_800_000_000;
const DAY: u64 = 86_400;

fn device(byte: u8) -> X25519KeyPair {
    X25519KeyPair::from_bytes([byte; 32])
}

// @internal
#[test]
fn each_side_opens_the_direction_the_other_reads_and_an_update_crosses_it() {
    let shared_key = [7u8; 32];
    let (alice, bob) = (device(1), device(2));
    let alice_public = *alice.public_key();
    let bob_public = *bob.public_key();

    let (mut alice_to_bob, bob_from_alice_seen_by_alice) =
        open_directions(&shared_key, &alice, &bob_public, NOW);
    let (bob_to_alice, mut bob_from_alice) = open_directions(&shared_key, &bob, &alice_public, NOW);

    assert_eq!(alice_to_bob.writes_in(), bob_from_alice.seed_epoch());
    assert_eq!(
        bob_to_alice.writes_in(),
        bob_from_alice_seen_by_alice.seed_epoch()
    );
    assert_ne!(alice_to_bob.writes_in(), bob_to_alice.writes_in());

    let update_key = [9u8; 32];
    let commitment = [3u8; 31];
    alice_to_bob.queue_update(&update_key, &commitment);
    let written = alice_to_bob.write(NOW);
    let read = bob_from_alice.read(&written, NOW + DAY);
    assert_eq!(read.len(), 1);
    assert_eq!(*read[0].update_key, update_key);
    assert_eq!(read[0].commitment, commitment);
    assert!(alice_to_bob.alarms().is_empty());
    assert!(bob_from_alice.alarms().is_empty());
}

// @internal
#[test]
fn a_device_linked_later_gets_directions_of_its_own() {
    let shared_key = [7u8; 32];
    let alice = device(1);
    let (bob_phone, bob_laptop) = (device(2), device(3));

    let (to_phone, _) = open_directions(&shared_key, &alice, bob_phone.public_key(), NOW);
    let (to_laptop, _) = open_directions(&shared_key, &alice, bob_laptop.public_key(), NOW);

    assert_ne!(to_phone.writes_in(), to_laptop.writes_in());
}

// @internal
#[test]
fn another_contacts_shared_key_opens_other_directions() {
    let (alice, bob) = (device(1), device(2));

    let (first, _) = open_directions(&[7u8; 32], &alice, bob.public_key(), NOW);
    let (second, _) = open_directions(&[8u8; 32], &alice, bob.public_key(), NOW);

    assert_ne!(first.writes_in(), second.writes_in());
}

fn storage_with_contact() -> (tempfile::TempDir, Storage, String) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("vauchi.db"), SymmetricKey::generate()).unwrap();
    let peer = Identity::create("Bob", 0);
    let contact = Contact::from_exchange(
        *peer.signing_public_key(),
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    storage.contacts().save_contact(&contact).unwrap();
    (dir, storage, contact.id().to_string())
}

fn peer(device_id: u8, key: &X25519KeyPair) -> PeerDevice {
    PeerDevice {
        device_id: [device_id; 32],
        exchange_public: *key.public_key(),
    }
}

// @internal
#[test]
fn a_contacts_directions_are_saved_once_per_peer_device() {
    let (_dir, storage, contact) = storage_with_contact();
    let shared_key = [7u8; 32];
    let own = device(1);
    let (phone, laptop) = (device(2), device(3));
    let peers = [peer(0xA0, &phone), peer(0xB0, &laptop)];

    let opened = ensure_directions(&storage, &contact, &shared_key, &own, &peers, NOW).unwrap();

    assert_eq!(opened, 2);
    let notices = storage.notices();
    for (device_id, key) in [([0xA0; 32], &phone), ([0xB0; 32], &laptop)] {
        let saved = notices
            .load(&contact, NoticeStateKind::Sender, &device_id)
            .unwrap()
            .expect("a saved sender");
        let (fresh, _) = open_directions(&shared_key, &own, key.public_key(), NOW);
        assert_eq!(
            Sender::load(&saved, NOW).unwrap().writes_in(),
            fresh.writes_in()
        );
        assert!(
            notices
                .load(&contact, NoticeStateKind::Receiver, &device_id)
                .unwrap()
                .is_some()
        );
    }
}

// A direction opened again would restart its chain and seal new
// plaintexts under keys it already used (ADR-019 amendment 2026-11-24).
// @internal
#[test]
fn opening_again_leaves_a_direction_that_has_moved_as_it_is() {
    let (_dir, storage, contact) = storage_with_contact();
    let shared_key = [7u8; 32];
    let own = device(1);
    let phone = device(2);
    let peers = [peer(0xA0, &phone)];
    ensure_directions(&storage, &contact, &shared_key, &own, &peers, NOW).unwrap();
    let notices = storage.notices();
    let saved = notices
        .load(&contact, NoticeStateKind::Sender, &[0xA0; 32])
        .unwrap()
        .unwrap();
    let mut sender = Sender::load(&saved, NOW).unwrap();
    sender.queue_update(&[9u8; 32], &[3u8; 31]);
    assert_eq!(sender.write(NOW).len(), 1);
    let moved = sender.save().unwrap();
    notices
        .save(&contact, NoticeStateKind::Sender, &[0xA0; 32], &moved)
        .unwrap();

    let opened =
        ensure_directions(&storage, &contact, &shared_key, &own, &peers, NOW + DAY).unwrap();

    assert_eq!(opened, 0);
    let after = notices
        .load(&contact, NoticeStateKind::Sender, &[0xA0; 32])
        .unwrap()
        .unwrap();
    assert_eq!(*after, *moved);
}
