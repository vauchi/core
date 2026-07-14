// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Alice A1/A2/A3 ↔ Bob B1/B2/B3 convergence using production card-update paths.

use vauchi_core::SymmetricKey;
use vauchi_core::api::Vauchi;
use vauchi_core::api::sync::process_card_updates;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::identity::{DeviceRegistry, Identity, RegistryBroadcast};
use vauchi_core::network::{compute_anonymous_id_for_device, current_epoch};

struct Party {
    devices: Vec<Vauchi>,
    registry: DeviceRegistry,
    signing_public_key: [u8; 32],
    card: ContactCard,
}

fn make_party(seed: [u8; 32], name: &str, email: &str) -> Party {
    let field = ContactField::new(FieldType::Email, "Email", email, 1);
    let field_id = field.id().to_string();
    let mut devices = Vec::new();
    for index in 0..3 {
        let mut device = Vauchi::in_memory().unwrap();
        let identity = Identity::from_device_link(
            seed,
            name.into(),
            index,
            format!("{name} device {}", index + 1),
            1,
        );
        device.set_identity(identity).unwrap();
        device.add_own_field(field.clone()).unwrap();
        device.set_own_field_public(&field_id).unwrap();
        devices.push(device);
    }
    let first_identity = devices[0].identity().unwrap();
    let mut registry = DeviceRegistry::new(
        first_identity.device_info().to_registered(&seed),
        first_identity.signing_keypair(),
    );
    for device in devices.iter().skip(1) {
        let identity = device.identity().unwrap();
        registry
            .add_device(
                identity.device_info().to_registered(&seed),
                first_identity.signing_keypair(),
            )
            .unwrap();
    }
    let signing_public_key = *first_identity.signing_public_key();
    let card = devices[0].own_card().unwrap().unwrap();
    Party {
        devices,
        registry,
        signing_public_key,
        card,
    }
}

fn connect_parties(alice: &mut Party, bob: &mut Party, relationship: &SymmetricKey) {
    let now = alice.devices[0].storage().clock().unix_seconds();
    let alice_broadcast = RegistryBroadcast::new(
        &alice.registry,
        alice.devices[0].identity().unwrap().signing_keypair(),
        now,
    );
    let bob_broadcast = RegistryBroadcast::new(
        &bob.registry,
        bob.devices[0].identity().unwrap().signing_keypair(),
        now,
    );

    for device in &alice.devices {
        let contact = Contact::from_exchange(
            bob.signing_public_key,
            bob.card.clone(),
            relationship.clone(),
            now,
        );
        let contact_id = contact.id().to_string();
        device.add_contact(contact).unwrap();
        device
            .storage()
            .device()
            .save_contact_device_registry(&contact_id, &bob_broadcast, &bob.signing_public_key, 60)
            .unwrap();
    }
    for device in &bob.devices {
        let contact = Contact::from_exchange(
            alice.signing_public_key,
            alice.card.clone(),
            relationship.clone(),
            now,
        );
        let contact_id = contact.id().to_string();
        device.add_contact(contact).unwrap();
        device
            .storage()
            .device()
            .save_contact_device_registry(
                &contact_id,
                &alice_broadcast,
                &alice.signing_public_key,
                60,
            )
            .unwrap();
    }
}

fn send_from_each_device(
    senders: &mut Party,
    receivers: &mut Party,
    relationship: &SymmetricKey,
    new_email: &str,
) {
    let mut sender_tokens = std::collections::HashSet::new();
    for sender in &mut senders.devices {
        let old_card = sender.own_card().unwrap().unwrap();
        let mut new_card = old_card.clone();
        let field_id = new_card.fields()[0].id().to_string();
        new_card
            .update_field_value(&field_id, new_email, 2)
            .unwrap();
        sender.update_own_card(&new_card).unwrap();
        let receiver_contact_id = hex::encode(receivers.signing_public_key);
        let updates = sender
            .prepare_card_updates_for_contact(&receiver_contact_id, &old_card, &new_card)
            .unwrap();
        assert_eq!(updates.len(), 3);

        let now = sender.storage().clock().unix_seconds();
        let sender_device_id = *sender.identity().unwrap().device_id();
        let token = hex::encode(compute_anonymous_id_for_device(
            relationship.as_bytes(),
            current_epoch(now),
            &sender_device_id,
        ));
        assert!(sender_tokens.insert(token.clone()));
        for (target_device_id, ciphertext) in updates {
            let relay_visible = String::from_utf8(ciphertext.clone()).unwrap();
            assert!(!relay_visible.contains(new_email));
            assert!(!relay_visible.contains(&hex::encode(senders.signing_public_key)));
            assert!(!relay_visible.contains(&hex::encode(sender_device_id)));
            assert!(!relay_visible.contains(&hex::encode(target_device_id)));
            let receiver = receivers
                .devices
                .iter()
                .find(|device| device.identity().unwrap().device_id() == &target_device_id)
                .expect("fan-out target is an active peer device");
            let result = process_card_updates(
                receiver.identity().unwrap(),
                receiver.storage(),
                vec![(token.clone(), ciphertext)],
            )
            .unwrap();
            assert_eq!(result.processed, 1);
            assert_eq!(result.skipped, 0);
        }
    }
    assert_eq!(sender_tokens.len(), 3);
}

fn assert_email(party: &Party, contact_key: &[u8; 32], expected: &str) {
    let contact_id = hex::encode(contact_key);
    for device in &party.devices {
        let contact = device.get_contact(&contact_id).unwrap().unwrap();
        assert_eq!(contact.card().fields()[0].value(), expected);
    }
}

fn send_two_versions_reordered(
    senders: &mut Party,
    receivers: &Party,
    relationship: &SymmetricKey,
) {
    for sender in &mut senders.devices {
        let old = sender.own_card().unwrap().unwrap();
        let field_id = old.fields()[0].id().to_string();
        let mut middle = old.clone();
        middle
            .update_field_value(&field_id, "middle@example", 3)
            .unwrap();
        sender.update_own_card(&middle).unwrap();
        let receiver_id = hex::encode(receivers.signing_public_key);
        let middle_updates = sender
            .prepare_card_updates_for_contact(&receiver_id, &old, &middle)
            .unwrap();

        let mut final_card = middle.clone();
        final_card
            .update_field_value(&field_id, "final@example", 4)
            .unwrap();
        sender.update_own_card(&final_card).unwrap();
        let final_updates = sender
            .prepare_card_updates_for_contact(&receiver_id, &middle, &final_card)
            .unwrap();

        let sender_device = *sender.identity().unwrap().device_id();
        let token = hex::encode(compute_anonymous_id_for_device(
            relationship.as_bytes(),
            current_epoch(sender.storage().clock().unix_seconds()),
            &sender_device,
        ));
        for ((middle_target, middle_ciphertext), (final_target, final_ciphertext)) in
            middle_updates.into_iter().zip(final_updates)
        {
            assert_eq!(middle_target, final_target);
            let receiver = receivers
                .devices
                .iter()
                .find(|device| device.identity().unwrap().device_id() == &final_target)
                .unwrap();

            // Offline relay catch-up may return later chain messages first.
            let newest = process_card_updates(
                receiver.identity().unwrap(),
                receiver.storage(),
                vec![(token.clone(), final_ciphertext.clone())],
            )
            .unwrap();
            assert_eq!(newest.processed, 1);

            // The skipped ratchet key decrypts this, then delta-version policy
            // rejects the stale state without rolling the card back.
            let stale = process_card_updates(
                receiver.identity().unwrap(),
                receiver.storage(),
                vec![(token.clone(), middle_ciphertext)],
            )
            .unwrap();
            assert_eq!(stale.skipped, 1);

            // Relay retry/duplication is harmless after session advancement.
            let duplicate = process_card_updates(
                receiver.identity().unwrap(),
                receiver.storage(),
                vec![(token.clone(), final_ciphertext)],
            )
            .unwrap();
            assert_eq!(duplicate.skipped, 1);
        }
    }
}

// @scenario: multi_device_sync :: Three devices per user exchange updates bidirectionally
#[test]
fn six_devices_converge_after_every_device_sends() {
    let mut alice = make_party([10u8; 32], "Alice", "alice@old.example");
    let mut bob = make_party([20u8; 32], "Bob", "bob@old.example");
    let relationship = SymmetricKey::from_bytes([30u8; 32]);
    connect_parties(&mut alice, &mut bob, &relationship);

    // The deterministic initiator endpoint sends first so every reverse chain
    // is bootstrapped. Identity ordering dominates the device tuple ordering.
    if alice.signing_public_key < bob.signing_public_key {
        send_from_each_device(&mut alice, &mut bob, &relationship, "alice@new.example");
        send_from_each_device(&mut bob, &mut alice, &relationship, "bob@new.example");
    } else {
        send_from_each_device(&mut bob, &mut alice, &relationship, "bob@new.example");
        send_from_each_device(&mut alice, &mut bob, &relationship, "alice@new.example");
    }

    assert_email(&bob, &alice.signing_public_key, "alice@new.example");
    assert_email(&alice, &bob.signing_public_key, "bob@new.example");
}

// @scenario: multi_device_sync :: Offline catch-up tolerates reorder and duplicate delivery
#[test]
fn six_devices_converge_under_reordering_and_duplicate_delivery() {
    let mut alice = make_party([70u8; 32], "Alice", "alice@old.example");
    let mut bob = make_party([80u8; 32], "Bob", "bob@old.example");
    let relationship = SymmetricKey::from_bytes([90u8; 32]);
    connect_parties(&mut alice, &mut bob, &relationship);

    // Bootstrap both directions for all nine pair sessions.
    if alice.signing_public_key < bob.signing_public_key {
        send_from_each_device(&mut alice, &mut bob, &relationship, "alice@bootstrap");
        send_from_each_device(&mut bob, &mut alice, &relationship, "bob@bootstrap");
    } else {
        send_from_each_device(&mut bob, &mut alice, &relationship, "bob@bootstrap");
        send_from_each_device(&mut alice, &mut bob, &relationship, "alice@bootstrap");
    }

    send_two_versions_reordered(&mut alice, &bob, &relationship);
    assert_email(&bob, &alice.signing_public_key, "final@example");
}
