// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A device the contact revoked cannot change the contact's card through
//! the genesis-sealed path, once any envelope has carried the registry that
//! revokes it (vauchi/private#1387).

use crate::common::helpers::create_vauchi_with_identity;
use vauchi_core::SymmetricKey;
use vauchi_core::api::{CardUpdateError, ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::exchange::genesis::GenesisEnvelope;
use vauchi_core::identity::{DeviceRegistry, Identity, RegistryBroadcast};
use vauchi_core::network::mailbox_token::current_day_epoch;
use vauchi_core::sync::delta::{CardDelta, CekWrappedPayload, VersionedPayload};

struct AliceDevice {
    identity: Identity,
    key: SigningKeyPair,
}

impl AliceDevice {
    fn linked(first: &Identity, index: u32, name: &str) -> Self {
        Self {
            identity: Identity::from_device_link(
                *first.master_seed(),
                "Alice".into(),
                index,
                name.into(),
                index.into(),
            ),
            key: SigningKeyPair::generate(),
        }
    }

    fn register_in(&self, registry: &mut DeviceRegistry, first: &Identity) {
        let mut entry = self
            .identity
            .device_info()
            .to_registered(first.master_seed());
        entry.signing_public_key = Some(*self.key.public_key().as_bytes());
        registry.add_device(entry, first.signing_keypair()).unwrap();
    }

    fn registry_broadcast(
        &self,
        registry: &DeviceRegistry,
        first: &Identity,
        now: u64,
    ) -> RegistryBroadcast {
        RegistryBroadcast::signed_by_device(
            registry,
            first.signing_keypair(),
            *self.identity.device_id(),
            &self.key,
            now,
        )
    }
}

struct World {
    a1: AliceDevice,
    a2: AliceDevice,
    bob: vauchi_core::Vauchi,
    alice_contact_id: String,
    shared_key: SymmetricKey,
    base_card: ContactCard,
}

fn world() -> World {
    let first = Identity::create("Alice", 0);
    let a1 = AliceDevice {
        identity: first,
        key: SigningKeyPair::generate(),
    };
    let a2 = AliceDevice::linked(&a1.identity, 1, "Alice A2");
    let bob = create_vauchi_with_identity("Bob");
    let shared_key = SymmetricKey::generate();
    let mut base_card = ContactCard::new("Alice");
    base_card
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "before@example.com",
            0,
        ))
        .unwrap();
    let alice = Contact::from_exchange(
        *a1.identity.signing_public_key(),
        base_card.clone(),
        shared_key.clone(),
        0,
    );
    let alice_contact_id = alice.id().to_string();
    bob.add_contact(alice).unwrap();
    World {
        a1,
        a2,
        bob,
        alice_contact_id,
        shared_key,
        base_card,
    }
}

impl World {
    fn now(&self) -> u64 {
        self.bob.storage().clock().unix_seconds()
    }

    fn registry_of_a1_and_a2(&self) -> DeviceRegistry {
        let mut registry = self
            .a1
            .identity
            .initial_device_registry_naming(*self.a1.key.public_key().as_bytes());
        self.a2.register_in(&mut registry, &self.a1.identity);
        registry
    }

    fn deliver(
        &self,
        sender: &AliceDevice,
        registry: &RegistryBroadcast,
        email: &str,
        version: u32,
        edited_at: u64,
    ) -> Result<ReceiveOutcome, CardUpdateError> {
        let bob_pk = *self.bob.identity().unwrap().signing_public_key();
        let now = edited_at;
        let mut edited = self.base_card.clone();
        edited
            .update_field_value(self.base_card.fields()[0].id(), email, now)
            .unwrap();
        let mut delta = CardDelta::compute(&self.base_card, &edited, now);
        delta.set_version(version);
        delta.sign(&sender.identity, &bob_pk);
        delta.sign_by_device(*sender.identity.device_id(), &sender.key);

        let delta_bytes = serde_json::to_vec(&delta).unwrap();
        let cek = ContentEncryptionKey::generate();
        let wrapped = CekWrappedPayload {
            cek: cek.to_bytes(),
            cek_ciphertext: cek.encrypt(&delta_bytes).unwrap(),
            signature: delta.signature,
            nonce: delta.nonce,
        };
        let (message, _) = GenesisEnvelope::seal(
            &self.shared_key,
            &sender.identity,
            &bob_pk,
            registry,
            current_day_epoch(now),
            &VersionedPayload::encode_cek(&wrapped),
        )
        .unwrap();
        process_single_card_update(
            self.bob.identity().unwrap(),
            self.bob.storage(),
            &self.alice_contact_id,
            &serde_json::to_vec(&message).unwrap(),
        )
    }

    fn stored_email(&self) -> String {
        self.bob
            .storage()
            .contacts()
            .load_contact(&self.alice_contact_id)
            .unwrap()
            .unwrap()
            .card()
            .fields()[0]
            .value()
            .to_string()
    }
}

// @scenario: device_management :: Lost device revocation
#[test]
fn a_revoked_device_cannot_change_the_card_through_a_genesis_envelope() {
    let w = world();
    let now = w.now();
    let with_a1 =
        w.a1.registry_broadcast(&w.registry_of_a1_and_a2(), &w.a1.identity, now);
    assert!(matches!(
        w.deliver(&w.a1, &with_a1, "a1@example.com", 1, now + 1),
        Ok(ReceiveOutcome::CardDelta)
    ));

    let a3 = AliceDevice::linked(&w.a1.identity, 2, "Alice A3");
    let mut registry = w.registry_of_a1_and_a2();
    a3.register_in(&mut registry, &w.a1.identity);
    registry
        .revoke_device(
            w.a1.identity.device_id(),
            w.a1.identity.signing_keypair(),
            now,
        )
        .unwrap();
    let without_a1 = a3.registry_broadcast(&registry, &w.a1.identity, now);
    match w.deliver(&a3, &without_a1, "a3@example.com", 1, now + 2) {
        Ok(ReceiveOutcome::CardDelta) => {}
        other => panic!("the relinked device's card was not applied: {other:?}"),
    }
    assert_eq!(w.stored_email(), "a3@example.com");

    for (carried, label) in [
        (&with_a1, "the older registry"),
        (&without_a1, "the revoking registry"),
    ] {
        match w.deliver(&w.a1, carried, "copy@example.com", u32::MAX, now + 3) {
            Err(CardUpdateError::DeviceSignatureInvalid) => {}
            other => {
                panic!("the revoked device's card, carrying {label}, was not refused: {other:?}")
            }
        }
    }
    assert_eq!(w.stored_email(), "a3@example.com");
    let held = w
        .bob
        .storage()
        .device()
        .load_contact_device_registry(&w.alice_contact_id)
        .unwrap()
        .expect("the receiver keeps the newest registry it was carried");
    assert_eq!(held.version(), without_a1.version());
}
