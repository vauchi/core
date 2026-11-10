// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact's pinned device keys persist per contact, sealed like the
//! registry they came from, and leave with the contact row
//! (vauchi/private#970, SC-62).

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::identity::PinnedDeviceKeys;
use vauchi_core::{Storage, SymmetricKey};

fn storage_with_contacts(names: &[&str]) -> (Storage, Vec<String>) {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let ids = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let contact = Contact::from_exchange(
                [i as u8 + 1; 32],
                ContactCard::new(name),
                SymmetricKey::generate(),
                0,
            );
            storage.contacts().save_contact(&contact).unwrap();
            contact.id().to_string()
        })
        .collect();
    (storage, ids)
}

fn pins_of(pairs: &[([u8; 32], [u8; 32])]) -> PinnedDeviceKeys {
    PinnedDeviceKeys::from_pairs(pairs.iter().copied())
}

// @scenario: device_sync :: Contact device registry persistence
// @internal
#[test]
fn pins_round_trip_per_contact() {
    let (storage, ids) = storage_with_contacts(&["Alice", "Bob"]);
    let alice_pins = pins_of(&[([1u8; 32], [11u8; 32]), ([2u8; 32], [22u8; 32])]);
    let bob_pins = pins_of(&[([3u8; 32], [33u8; 32])]);

    assert_eq!(
        storage.device().load_contact_device_pins(&ids[0]).unwrap(),
        None
    );
    storage
        .device()
        .save_contact_device_pins(&ids[0], &alice_pins)
        .unwrap();
    storage
        .device()
        .save_contact_device_pins(&ids[1], &bob_pins)
        .unwrap();

    let loaded = storage.device().load_contact_device_pins(&ids[0]).unwrap();
    assert_eq!(loaded, Some(alice_pins.clone()));
    assert_eq!(loaded.unwrap().pinned(&[2u8; 32]), Some([22u8; 32]));
    assert_eq!(
        storage.device().load_contact_device_pins(&ids[1]).unwrap(),
        Some(bob_pins),
        "pins are per contact"
    );
}

// @scenario: device_sync :: Contact device registry persistence
// @internal
#[test]
fn a_later_save_replaces_the_pins_and_the_row_is_sealed() {
    let (storage, ids) = storage_with_contacts(&["Alice"]);
    storage
        .device()
        .save_contact_device_pins(&ids[0], &pins_of(&[([1u8; 32], [11u8; 32])]))
        .unwrap();
    let grown = pins_of(&[([1u8; 32], [11u8; 32]), ([2u8; 32], [22u8; 32])]);

    storage
        .device()
        .save_contact_device_pins(&ids[0], &grown)
        .unwrap();

    assert_eq!(
        storage.device().load_contact_device_pins(&ids[0]).unwrap(),
        Some(grown)
    );
    let raw: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT pins_encrypted FROM contact_device_key_pins WHERE contact_id = ?1",
            [&ids[0]],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !raw.windows(32).any(|w| w == [22u8; 32]),
        "a pinned key must not be readable from the row"
    );
}

// @scenario: device_sync :: Contact device registry persistence
// @internal
#[test]
fn pins_leave_with_the_contact() {
    let (storage, ids) = storage_with_contacts(&["Alice"]);
    storage
        .device()
        .save_contact_device_pins(&ids[0], &pins_of(&[([1u8; 32], [11u8; 32])]))
        .unwrap();

    assert!(storage.delete_contact(&ids[0]).unwrap());

    assert_eq!(
        storage.device().load_contact_device_pins(&ids[0]).unwrap(),
        None
    );
}

// @scenario: device_sync :: Contact device registry persistence
// @internal
#[test]
fn pins_for_an_unknown_contact_are_refused() {
    let (storage, _) = storage_with_contacts(&[]);

    let result = storage
        .device()
        .save_contact_device_pins("nobody", &pins_of(&[([1u8; 32], [11u8; 32])]));

    assert!(result.is_err(), "the pins row references the contact row");
}
