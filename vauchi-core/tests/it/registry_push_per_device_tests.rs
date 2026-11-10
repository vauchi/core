// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact accepts a registry broadcast only when the device it names
//! as signer is active and signed it with the key the contact pinned for
//! it (vauchi/private#970, SC-62). Everything goes through the public
//! receive path a relay delivery takes.

use crate::common::device_sync::device_signed_broadcast;
use crate::common::helpers::create_vauchi_with_identity;
use vauchi_core::SymmetricKey;
use vauchi_core::api::{CardUpdateError, ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::identity::{DeviceRegistry, RegisteredDevice, RegistryBroadcast};
use vauchi_core::sync::delta::VersionedPayload;
use vauchi_core::sync::registry_activation::RegistryPushPayload;

struct Party {
    alice: vauchi_core::Vauchi,
    bob: vauchi_core::Vauchi,
    bob_at_alice: String,
    alice_at_bob: String,
    nonce: std::cell::Cell<u8>,
}

fn two_parties() -> Party {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let shared = SymmetricKey::generate();
    let bob_contact = Contact::from_exchange(bob_pk, ContactCard::new("Bob"), shared.clone(), 0);
    let bob_at_alice = bob_contact.id().to_string();
    alice.add_contact(bob_contact).unwrap();
    let alice_contact =
        Contact::from_exchange(alice_pk, ContactCard::new("Alice"), shared.clone(), 0);
    let alice_at_bob = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();
    let alice_dh = X3DHKeyPair::generate();
    let bob_ratchet =
        DoubleRatchetState::initialize_initiator(&shared, *alice_dh.public_key()).unwrap();
    let alice_ratchet = DoubleRatchetState::initialize_responder(&shared, alice_dh);
    alice
        .storage()
        .ratchets()
        .save_ratchet_state(&bob_at_alice, &alice_ratchet, false)
        .unwrap();
    bob.storage()
        .ratchets()
        .save_ratchet_state(&alice_at_bob, &bob_ratchet, true)
        .unwrap();
    Party {
        alice,
        bob,
        bob_at_alice,
        alice_at_bob,
        nonce: std::cell::Cell::new(0),
    }
}

impl Party {
    fn bob_device_key(&self) -> SigningKeyPair {
        self.bob
            .storage()
            .device()
            .load_or_create_device_signing_key()
            .unwrap()
    }

    fn bob_device_id(&self) -> [u8; 32] {
        *self.bob.identity().unwrap().device_id()
    }

    /// Bob's phone entry naming its stored key, as his saved registry does.
    fn bob_registry(&self) -> DeviceRegistry {
        let key = *self.bob_device_key().public_key().as_bytes();
        self.bob
            .identity()
            .unwrap()
            .initial_device_registry_naming(key)
    }

    fn now(&self) -> u64 {
        self.bob.storage().clock().unix_seconds()
    }

    /// Delivers a broadcast to Alice the way a relay push does.
    fn push_to_alice(
        &self,
        broadcast: &RegistryBroadcast,
    ) -> Result<ReceiveOutcome, CardUpdateError> {
        self.nonce.set(self.nonce.get() + 1);
        let push =
            RegistryPushPayload::new([self.nonce.get(); 32], broadcast.to_json().into_bytes())
                .unwrap();
        let payload = VersionedPayload::encode_registry_push(&push);
        let (mut ratchet, is_init) = self
            .bob
            .storage()
            .ratchets()
            .load_ratchet_state(&self.alice_at_bob)
            .unwrap()
            .unwrap();
        let message = ratchet.encrypt(&payload).unwrap();
        self.bob
            .storage()
            .ratchets()
            .save_ratchet_state(&self.alice_at_bob, &ratchet, is_init)
            .unwrap();
        let blob = serde_json::to_vec(&message).unwrap();
        let alice_identity = self.alice.identity().unwrap();
        process_single_card_update(
            alice_identity,
            self.alice.storage(),
            &self.bob_at_alice,
            &blob,
        )
    }

    fn alice_pin_for(&self, device_id: &[u8; 32]) -> Option<[u8; 32]> {
        self.alice
            .storage()
            .device()
            .load_contact_device_pins(&self.bob_at_alice)
            .unwrap()
            .and_then(|pins| pins.pinned(device_id))
    }
}

fn tablet_entry(key: &SigningKeyPair) -> RegisteredDevice {
    RegisteredDevice {
        device_id: [7u8; 32],
        exchange_public_key: [8u8; 32],
        signing_public_key: Some(*key.public_key().as_bytes()),
        device_name: "tablet".into(),
        created_at: 0,
        revoked: false,
        revoked_at: None,
        last_sync_at: None,
    }
}

fn refused_with(result: Result<ReceiveOutcome, CardUpdateError>, cause: &str) {
    match result {
        Err(CardUpdateError::InvalidPayload(message)) => assert!(
            message.contains(cause),
            "refused for another reason: {message}"
        ),
        other => panic!("expected a refusal naming {cause:?}, got {other:?}"),
    }
}

// @scenario: multi_device_sync :: A received push persists the registry and asks for an ack
// @internal
#[test]
fn a_device_signed_push_is_accepted_and_its_key_pinned() {
    let p = two_parties();
    let honest = device_signed_broadcast(&p.bob);

    let outcome = p.push_to_alice(&honest).unwrap();

    assert!(matches!(outcome, ReceiveOutcome::RegistryPushReceived(_)));
    assert_eq!(
        p.alice_pin_for(&p.bob_device_id()),
        Some(*p.bob_device_key().public_key().as_bytes()),
        "Alice pinned Bob's phone key on first sight"
    );
}

// @scenario: multi_device_sync :: A received push persists the registry and asks for an ack
// @internal
#[test]
fn a_broadcast_signed_by_the_identity_alone_is_refused() {
    let p = two_parties();
    let identity_only = RegistryBroadcast::new(
        &p.bob_registry(),
        p.bob.identity().unwrap().signing_keypair(),
        p.now(),
    );

    let result = p.push_to_alice(&identity_only);

    refused_with(result, "Invalid device signature");
    assert_eq!(
        p.alice_pin_for(&p.bob_device_id()),
        None,
        "a refused broadcast pins nothing"
    );
}

// @scenario: multi_device_sync :: A received push persists the registry and asks for an ack
// @internal
#[test]
fn a_linked_device_cannot_sign_a_broadcast_as_another_device() {
    let p = two_parties();
    let identity_key = p.bob.identity().unwrap().signing_keypair().clone();
    let tablet_key = SigningKeyPair::generate();
    let mut registry = p.bob_registry();
    registry
        .add_device(tablet_entry(&tablet_key), &identity_key)
        .unwrap();
    let phone_claims_tablet = RegistryBroadcast::signed_by_device(
        &registry,
        &identity_key,
        [7u8; 32],
        &p.bob_device_key(),
        p.now(),
    );

    let result = p.push_to_alice(&phone_claims_tablet);

    refused_with(result, "Invalid device signature");
}

// @scenario: multi_device_sync :: A received push persists the registry and asks for an ack
// @internal
#[test]
fn a_signer_that_is_not_an_active_device_is_refused() {
    let p = two_parties();
    let identity_key = p.bob.identity().unwrap().signing_keypair().clone();
    let stranger = SigningKeyPair::generate();
    let as_unknown_device = RegistryBroadcast::signed_by_device(
        &p.bob_registry(),
        &identity_key,
        [7u8; 32],
        &stranger,
        p.now(),
    );

    let result = p.push_to_alice(&as_unknown_device);

    refused_with(result, "not an active device");
}

// @scenario: multi_device_sync :: A received push persists the registry and asks for an ack
// @internal
#[test]
fn a_known_devices_key_cannot_change_even_with_the_identity_key() {
    let p = two_parties();
    let identity_key = p.bob.identity().unwrap().signing_keypair().clone();
    let honest = device_signed_broadcast(&p.bob);
    p.push_to_alice(&honest).unwrap();
    let rekey = SigningKeyPair::generate();
    let mut rewritten = p.bob.identity().unwrap().initial_device_registry();
    rewritten
        .name_device_key(
            &p.bob_device_id(),
            *rekey.public_key().as_bytes(),
            &identity_key,
        )
        .unwrap();
    assert!(
        rewritten.version() > p.bob_registry().version(),
        "newer than the pinned one"
    );
    let rekeyed = RegistryBroadcast::signed_by_device(
        &rewritten,
        &identity_key,
        p.bob_device_id(),
        &rekey,
        p.now(),
    );

    let result = p.push_to_alice(&rekeyed);

    refused_with(result, "signing key changed");
    assert_eq!(
        p.alice_pin_for(&p.bob_device_id()),
        Some(*p.bob_device_key().public_key().as_bytes()),
        "the first key stays pinned"
    );
}

// @scenario: multi_device_sync :: A received push persists the registry and asks for an ack
// @internal
#[test]
fn a_removed_device_cannot_sign_and_its_pin_is_kept() {
    let p = two_parties();
    let identity_key = p.bob.identity().unwrap().signing_keypair().clone();
    let tablet_key = SigningKeyPair::generate();
    let mut registry = p.bob_registry();
    registry
        .add_device(tablet_entry(&tablet_key), &identity_key)
        .unwrap();
    let with_tablet = RegistryBroadcast::signed_by_device(
        &registry,
        &identity_key,
        [7u8; 32],
        &tablet_key,
        p.now(),
    );
    p.push_to_alice(&with_tablet).unwrap();
    assert_eq!(
        p.alice_pin_for(&[7u8; 32]),
        Some(*tablet_key.public_key().as_bytes())
    );
    registry
        .revoke_device(&[7u8; 32], &identity_key, p.now())
        .unwrap();

    let removed_signs = RegistryBroadcast::signed_by_device(
        &registry,
        &identity_key,
        [7u8; 32],
        &tablet_key,
        p.now(),
    );
    let result = p.push_to_alice(&removed_signs);
    refused_with(result, "not an active device");

    let (phone_id, phone_key, now) = (p.bob_device_id(), p.bob_device_key(), p.now());
    let phone_signs =
        RegistryBroadcast::signed_by_device(&registry, &identity_key, phone_id, &phone_key, now);
    p.push_to_alice(&phone_signs).unwrap();
    assert_eq!(
        p.alice_pin_for(&[7u8; 32]),
        Some(*tablet_key.public_key().as_bytes()),
        "a removed device's key stays pinned so it cannot come back under that id"
    );
}
