// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(all(feature = "network-http", feature = "testing"))]

use vauchi_core::api::{Vauchi, VauchiConfig, VauchiSyncOutcome};
use vauchi_core::clock::FakeClock;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ProtocolState;
use vauchi_core::network::OhttpClient;
use vauchi_core::rng::OsSecureRng;
use vauchi_core::{Contact, ContactField, FieldType};
use vauchi_protocol::ohttp_key::window_of;

use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{NOW, SignedGateway};

struct Pair {
    alice: Vauchi,
    bob: Vauchi,
    bob_at_alice: String,
    alice_at_bob: String,
}

fn client(name: &str, application: &MockRelay, outer: &MockRelay, offline: bool) -> Vauchi {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay(application.url(), SignedGateway::new(0x41).anchor())
        .with_ohttp_relay_url(outer.url());
    config.offline = offline;
    let clock =
        FakeClock::new(std::time::UNIX_EPOCH + std::time::Duration::from_secs(NOW)).shared();
    let mut vauchi =
        Vauchi::new_with(config, clock, OsSecureRng::shared(), None).expect("create Vauchi");
    vauchi.create_identity(name).expect("identity");
    let field = ContactField::new(FieldType::Email, "work", "zero@example.test", 0);
    let field_id = field.id().to_string();
    vauchi.add_own_field(field).expect("field");
    vauchi
        .set_own_field_public(&field_id)
        .expect("public field");
    vauchi.set_ohttp_key_for_testing(OhttpClient::new(outer.ohttp_key_config()).expect("key"));
    vauchi
}

fn exchange_cards(alice: &Vauchi, bob: &Vauchi) -> (String, String) {
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let alice_card = alice.own_card().unwrap().unwrap();
    let bob_card = bob.own_card().unwrap().unwrap();
    let mut alice_session = MultiStageSession::new(serde_json::to_vec(&alice_card).unwrap());
    let mut bob_session = MultiStageSession::new(serde_json::to_vec(&bob_card).unwrap());
    let alice_init = alice_session.get_display_qr().unwrap();
    let bob_init = bob_session.get_display_qr().unwrap();
    alice_session.process_scanned_qr(&bob_init.data);
    bob_session.process_scanned_qr(&alice_init.data);
    for _ in 0..100 {
        let alice_qr = alice_session.get_display_qr();
        let bob_qr = bob_session.get_display_qr();
        if let Some(qr) = &alice_qr {
            bob_session.process_scanned_qr(&qr.data);
        }
        if let Some(qr) = &bob_qr {
            alice_session.process_scanned_qr(&qr.data);
        }
        if matches!(alice_session.get_state(), ProtocolState::Finalized)
            && matches!(bob_session.get_state(), ProtocolState::Finalized)
        {
            break;
        }
    }
    assert!(matches!(
        alice_session.get_state(),
        ProtocolState::Finalized
    ));
    assert!(matches!(bob_session.get_state(), ProtocolState::Finalized));
    let shared = alice_session.get_transport_key().expect("transport key");
    assert_eq!(shared, bob_session.get_transport_key().unwrap());
    let (alice_ratchet, alice_is_initiator) = alice_session
        .build_exchange_ratchet(&alice_pk, &bob_pk)
        .expect("alice ratchet");
    let (bob_ratchet, bob_is_initiator) = bob_session
        .build_exchange_ratchet(&bob_pk, &alice_pk)
        .expect("bob ratchet");
    let bob_at_alice =
        Contact::from_exchange(bob_pk, bob_card, SymmetricKey::from_bytes(shared), 0);
    let alice_at_bob =
        Contact::from_exchange(alice_pk, alice_card, SymmetricKey::from_bytes(shared), 0);
    let bob_id = bob_at_alice.id().to_string();
    let alice_id = alice_at_bob.id().to_string();
    alice
        .save_exchanged_contact(&bob_at_alice, &alice_ratchet, alice_is_initiator)
        .expect("save Bob");
    bob.save_exchanged_contact(&alice_at_bob, &bob_ratchet, bob_is_initiator)
        .expect("save Alice");
    (bob_id, alice_id)
}

fn start_pair(offline: bool) -> (Pair, MockRelay, MockRelay) {
    let application = MockRelay::start();
    let outer = MockRelay::start();
    outer.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
    let clock =
        FakeClock::new(std::time::UNIX_EPOCH + std::time::Duration::from_secs(NOW)).shared();
    outer.enable_gateway_mailboxes(clock, 64);
    let alice = client("Alice", &application, &outer, offline);
    let bob = client("Bob", &application, &outer, offline);
    let (bob_at_alice, alice_at_bob) = exchange_cards(&alice, &bob);
    (
        Pair {
            alice,
            bob,
            bob_at_alice,
            alice_at_bob,
        },
        application,
        outer,
    )
}

fn set_offline(pair: &mut Pair, offline: bool) {
    pair.alice.config_mut().offline = offline;
    pair.bob.config_mut().offline = offline;
}

fn request_count(application: &MockRelay, outer: &MockRelay) -> usize {
    application.received().len() + outer.received().len()
}

fn sync_pair(pair: &mut Pair) {
    for index in 0..3 {
        pair.alice.set_next_sync_allowed(std::time::Instant::now());
        pair.bob.set_next_sync_allowed(std::time::Instant::now());
        let alice_outcome = pair.alice.sync().expect("Alice sync");
        let bob_outcome = pair.bob.sync().expect("Bob sync");
        if index == 0 {
            match alice_outcome {
                VauchiSyncOutcome::Ok { sent, .. } if sent > 0 => {}
                other => panic!("Alice sent no update: {other:?}"),
            }
            match bob_outcome {
                VauchiSyncOutcome::Ok { received, .. } if received > 0 => {}
                other => panic!("Bob received no update: {other:?}"),
            }
        }
    }
}

fn assert_email(pair: &Pair, expected: &str) {
    let contact = pair.bob.get_contact(&pair.alice_at_bob).unwrap().unwrap();
    let values: Vec<_> = contact
        .card()
        .fields()
        .iter()
        .filter(|field| field.label() == "work")
        .map(|field| field.value())
        .collect();
    assert_eq!(values, [expected], "received card: {:?}", contact.card());
}

fn update(
    pair: &mut Pair,
    application: &MockRelay,
    outer: &MockRelay,
    offline: bool,
    value: &str,
    updated_at: u64,
) {
    set_offline(pair, offline);
    let before = request_count(application, outer);
    let old = pair.alice.own_card().unwrap().unwrap();
    let field_id = old
        .fields()
        .iter()
        .find(|field| field.label() == "work")
        .unwrap()
        .id()
        .to_string();
    let mut new = old.clone();
    new.update_field_value(&field_id, value, updated_at)
        .unwrap();
    pair.alice.update_own_card(&new).unwrap();
    if offline {
        let ids = exchange_cards(&pair.alice, &pair.bob);
        assert_eq!(ids, (pair.bob_at_alice.clone(), pair.alice_at_bob.clone()));
        assert!(matches!(
            pair.alice.sync().unwrap(),
            VauchiSyncOutcome::NotConnected
        ));
        assert!(matches!(
            pair.bob.sync().unwrap(),
            VauchiSyncOutcome::NotConnected
        ));
        assert_eq!(request_count(application, outer), before);
    } else {
        sync_pair(pair);
    }
    assert_email(pair, value);
}

fn run_alternating(start_offline: bool) -> Pair {
    let (mut pair, application, outer) = start_pair(start_offline);
    for (index, value) in [
        "one@example.test",
        "two@example.test",
        "three@example.test",
        "four@example.test",
    ]
    .into_iter()
    .enumerate()
    {
        let offline = if index == 0 {
            start_offline
        } else if start_offline {
            index % 2 == 0
        } else {
            index % 2 == 1
        };
        update(
            &mut pair,
            &application,
            &outer,
            offline,
            value,
            index as u64 + 1,
        );
    }
    pair
}

// @scenario: sync_updates :: Updates converge after switching from local to relay
#[test]
fn local_to_relay_and_alternating_updates() {
    let pair = run_alternating(true);
    assert_email(&pair, "four@example.test");
}

// @scenario: sync_updates :: Updates converge after switching from relay to local
#[test]
fn relay_to_local_and_alternating_updates() {
    let pair = run_alternating(false);
    assert_email(&pair, "four@example.test");
}
