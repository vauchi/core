// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Two parties with a shared ratchet session, where Bob's device keys are
//! in the test's hands, for the per-device signing tests
//! (vauchi/private#970, SC-62).

use super::helpers::create_vauchi_with_identity;
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

pub struct Party {
    pub alice: vauchi_core::Vauchi,
    pub bob: vauchi_core::Vauchi,
    pub bob_at_alice: String,
    pub alice_at_bob: String,
    pub nonce: std::cell::Cell<u8>,
}

pub fn two_parties() -> Party {
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
    pub fn bob_device_key(&self) -> SigningKeyPair {
        self.bob
            .storage()
            .device()
            .load_or_create_device_signing_key()
            .unwrap()
    }

    pub fn bob_device_id(&self) -> [u8; 32] {
        *self.bob.identity().unwrap().device_id()
    }

    /// Bob's phone entry naming its stored key, as his saved registry does.
    pub fn bob_registry(&self) -> DeviceRegistry {
        let key = *self.bob_device_key().public_key().as_bytes();
        self.bob
            .identity()
            .unwrap()
            .initial_device_registry_naming(key)
    }

    pub fn now(&self) -> u64 {
        self.bob.storage().clock().unix_seconds()
    }

    /// Delivers a broadcast to Alice the way a relay push does.
    pub fn push_to_alice(
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

    pub fn alice_pin_for(&self, device_id: &[u8; 32]) -> Option<[u8; 32]> {
        self.alice
            .storage()
            .device()
            .load_contact_device_pins(&self.bob_at_alice)
            .unwrap()
            .and_then(|pins| pins.pinned(device_id))
    }
}

pub fn tablet_entry(key: &SigningKeyPair) -> RegisteredDevice {
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

pub fn refused_with(result: Result<ReceiveOutcome, CardUpdateError>, cause: &str) {
    match result {
        Err(CardUpdateError::InvalidPayload(message)) => assert!(
            message.contains(cause),
            "refused for another reason: {message}"
        ),
        other => panic!("expected a refusal naming {cause:?}, got {other:?}"),
    }
}
