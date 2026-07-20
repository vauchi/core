// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Step-1 pinning probes for the exchange/sync/device-link
//! consolidation plan (`_private` planning,
//! 2026-07-20-exchange-sync-devicelink-consolidation-plan).
//!
//! These are characterization tests: they pin behavior the Step-2/3
//! refactors must preserve. Every test here must stay green through
//! the consolidation; a red is a discovery, not a flake.
//!
//! - U3: the hand-rolled ratchet-init recipe in the BLE persist path
//!   (`vauchi-app` `ble_handshake.rs`) and the Link completion path
//!   (`api/vauchi/import.rs`) is functionally equivalent to the shared
//!   `bootstrap_exchange_ratchet` helper, so the planned substitution
//!   is behavior-preserving.
//! - U4: an unknown `ExchangeTransport` variant fails closed on
//!   decode — adding a variant requires tolerant readers to ship
//!   before any writer.
//! - U6: a sync cycle with no traffic does not mutate registered
//!   ratchet state — pins `SyncController`'s pass-through ratchet map
//!   as a no-op so its planned removal is provably safe.

use std::sync::Arc;

use proptest::prelude::*;
use vauchi_core::api::*;
use vauchi_core::crypto::{DoubleRatchetState, SymmetricKey};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::exchange::key_order;
use vauchi_core::exchange::link_mode::{derive_link_shared_key, serialize_card_payload_v2};
use vauchi_core::exchange::ratchet_bootstrap::bootstrap_exchange_ratchet;
use vauchi_core::network::{MockTransport, RelayClientConfig, TransportConfig};
use vauchi_core::rng::OsSecureRng;
use vauchi_core::*;

// ---------------------------------------------------------------------------
// U3 — hand-rolled init recipe ≡ bootstrap_exchange_ratchet
// ---------------------------------------------------------------------------

/// Drives an initiator-first ping-pong across a DH ratchet step and
/// asserts every message round-trips.
fn assert_bidirectional_interop(
    initiator: &mut DoubleRatchetState,
    responder: &mut DoubleRatchetState,
) {
    let m1 = initiator.encrypt(b"probe-1").expect("initiator encrypts");
    assert_eq!(
        responder.decrypt(&m1).expect("responder decrypts m1"),
        b"probe-1".to_vec()
    );
    let m2 = responder.encrypt(b"probe-2").expect("responder encrypts");
    assert_eq!(
        initiator.decrypt(&m2).expect("initiator decrypts m2"),
        b"probe-2".to_vec()
    );
    // Third message crosses a fresh DH ratchet step on the initiator side.
    let m3 = initiator
        .encrypt(b"probe-3")
        .expect("initiator encrypts m3");
    assert_eq!(
        responder.decrypt(&m3).expect("responder decrypts m3"),
        b"probe-3".to_vec()
    );
}

proptest! {
    // @internal
    #[test]
    fn hand_rolled_init_recipe_matches_bootstrap_helper_for_any_identities(
        our_id in any::<[u8; 32]>(),
        their_id in any::<[u8; 32]>(),
        shared_bytes in any::<[u8; 32]>(),
        our_x3dh_seed in any::<[u8; 32]>(),
        their_x3dh_seed in any::<[u8; 32]>(),
    ) {
        prop_assume!(our_id != their_id);
        prop_assume!(our_x3dh_seed != their_x3dh_seed);

        let shared = SymmetricKey::from_bytes(shared_bytes);
        let our_x3dh = X3DHKeyPair::from_bytes(our_x3dh_seed);
        let their_x3dh = X3DHKeyPair::from_bytes(their_x3dh_seed);

        // The exact recipe hand-rolled by the BLE persist path
        // (ble_handshake.rs: `is_initiator = our_identity < their_identity`,
        // initiator keys off the peer's exchange key, responder off our
        // own X3DH keypair) and by Link completion (import.rs, same shape).
        let manual_is_initiator = our_id < their_id;
        let manual = if manual_is_initiator {
            DoubleRatchetState::initialize_initiator(&shared, *their_x3dh.public_key())
                .expect("manual initiator init")
        } else {
            DoubleRatchetState::initialize_responder(
                &shared,
                X3DHKeyPair::from_bytes(*our_x3dh.secret_bytes()),
            )
        };

        // The shared helper, built for the PEER side with mirrored inputs.
        let (helper_peer, peer_is_initiator) = bootstrap_exchange_ratchet(
            &shared,
            &their_id,
            &our_id,
            Some(*our_x3dh.public_key()),
            Some(X3DHKeyPair::from_bytes(*their_x3dh.secret_bytes())),
        )
        .expect("helper bootstrap");

        // Role decisions must complement each other and agree with the
        // canonical rule the helper encodes.
        prop_assert_eq!(manual_is_initiator, !peer_is_initiator);
        prop_assert_eq!(
            manual_is_initiator,
            key_order::is_initiator(&our_id, &their_id)
        );

        // Functional equivalence: the manual side and the helper side
        // form a working bidirectional channel.
        let (mut init_side, mut resp_side) = if manual_is_initiator {
            (manual, helper_peer)
        } else {
            (helper_peer, manual)
        };
        let m1 = init_side.encrypt(b"probe-1").expect("encrypt m1");
        prop_assert_eq!(resp_side.decrypt(&m1).expect("decrypt m1"), b"probe-1".to_vec());
        let m2 = resp_side.encrypt(b"probe-2").expect("encrypt m2");
        prop_assert_eq!(init_side.decrypt(&m2).expect("decrypt m2"), b"probe-2".to_vec());
        let m3 = init_side.encrypt(b"probe-3").expect("encrypt m3");
        prop_assert_eq!(resp_side.decrypt(&m3).expect("decrypt m3"), b"probe-3".to_vec());
    }
}

// @internal
#[test]
fn link_persisted_ratchet_interoperates_with_helper_built_peer() {
    // Alice completes Bob's v2 bootstrap through the REAL Link path
    // (hand-rolled init inside `complete_link_exchange`); Bob's side is
    // built with the shared helper. If the two interoperate, swapping
    // the Link internals to the helper cannot break live channels.
    let mut alice = Vauchi::in_memory().expect("alice vauchi");
    alice.create_identity("Alice").expect("alice identity");
    let mut bob = Vauchi::in_memory().expect("bob vauchi");
    bob.create_identity("Bob").expect("bob identity");

    let alice_id = *alice
        .identity()
        .expect("alice identity exists")
        .signing_public_key();
    let bob_id = *bob
        .identity()
        .expect("bob identity exists")
        .signing_public_key();

    let alice_x3dh = X3DHKeyPair::generate();
    let bob_x3dh = X3DHKeyPair::generate();

    let bob_payload = {
        let identity = bob.identity().expect("bob identity");
        let card = vauchi_core::contact_card::ContactCard::new(identity.display_name());
        serialize_card_payload_v2(
            &bob_id,
            identity.signing_keypair(),
            bob_x3dh.public_key(),
            "https://relay.bob.example",
            &card,
        )
    };

    let contact_id = alice
        .complete_link_exchange(&bob_payload, &alice_x3dh)
        .expect("alice completes bob's bootstrap");

    let (alice_ratchet, alice_is_initiator) = alice
        .storage()
        .ratchets()
        .load_ratchet_state(&contact_id)
        .expect("load ok")
        .expect("alice persisted a ratchet");
    assert_eq!(
        alice_is_initiator,
        key_order::is_initiator(&alice_id, &bob_id),
        "persisted role flag must match the canonical role rule"
    );

    // Shape-(c) characterization (consolidation Step 1): Link persists an
    // Exchanged (live) contact stamped `Link`, via `add_contact` +
    // `save_exchange_ratchet` behind the never-rekey guard (pinned by
    // `complete_link_exchange_tests::v2_completion_is_idempotent_and_keeps_the_existing_channel`).
    let contact = alice
        .get_contact(&contact_id)
        .expect("get ok")
        .expect("link contact exists");
    assert_eq!(
        contact.exchange_transport(),
        Some(ExchangeTransport::Link),
        "Link persist stamps its transport"
    );
    assert!(
        contact.kind().exchanged_data().is_some(),
        "Link v2 persists an Exchanged (live) contact, not an import"
    );

    // Bob's side of the same channel, built with the shared helper.
    let shared = derive_link_shared_key(&bob_x3dh, alice_x3dh.public_key())
        .expect("bob derives the commutative link key");
    let (bob_ratchet, bob_is_initiator) = bootstrap_exchange_ratchet(
        &shared,
        &bob_id,
        &alice_id,
        Some(*alice_x3dh.public_key()),
        Some(X3DHKeyPair::from_bytes(*bob_x3dh.secret_bytes())),
    )
    .expect("helper bootstrap for bob");
    assert_ne!(
        alice_is_initiator, bob_is_initiator,
        "exactly one side is the initiator"
    );

    let (mut init_side, mut resp_side) = if alice_is_initiator {
        (alice_ratchet, bob_ratchet)
    } else {
        (bob_ratchet, alice_ratchet)
    };
    assert_bidirectional_interop(&mut init_side, &mut resp_side);
}

// ---------------------------------------------------------------------------
// U4 — unknown ExchangeTransport variant fails closed on decode
// ---------------------------------------------------------------------------

// @internal
#[test]
fn unknown_exchange_transport_variant_fails_closed_on_decode() {
    // Wire/storage shape today (`exchange_types.rs`): snake_case,
    // `#[non_exhaustive]`, NO `#[serde(other)]` fallback. Contact rows
    // carry this enum inside the encrypted card JSON
    // (`storage/stores/contact.rs::contact_to_row`), so an old reader
    // meeting a future variant fails the whole contact decode.
    // Consequence pinned here: adding `MultiStage` (consolidation plan
    // Step 2) must ship tolerant READERS at least one release before
    // any writer stamps it.
    assert_eq!(
        serde_json::to_string(&ExchangeTransport::Ble).expect("serialize"),
        "\"ble\"",
        "current wire spelling is snake_case"
    );
    assert_eq!(
        serde_json::from_str::<ExchangeTransport>("\"Ble\"").expect("legacy alias decodes"),
        ExchangeTransport::Ble,
        "legacy CamelCase alias must keep decoding"
    );
    // Step 2e shipped reader support: `multi_stage` decodes even though
    // no persist path writes it yet (writers ship a release later).
    assert_eq!(
        serde_json::from_str::<ExchangeTransport>("\"multi_stage\"").expect("reader support"),
        ExchangeTransport::MultiStage,
        "MultiStage readers ship ahead of writers"
    );
    let unknown = serde_json::from_str::<ExchangeTransport>("\"carrier_pigeon\"");
    assert!(
        unknown.is_err(),
        "an unknown transport variant must fail decode (fail closed), got {unknown:?}"
    );
}

// ---------------------------------------------------------------------------
// U6 — a traffic-free sync cycle does not mutate registered ratchets
// ---------------------------------------------------------------------------

// @internal
#[test]
fn sync_cycle_without_traffic_does_not_mutate_registered_ratchets() {
    // `sync_http.rs` re-saves every ratchet handed back by
    // `into_ratchets()` after the send phase. This pins the claim that
    // the controller never advances them (advance happens upstream at
    // queue/receive time), so the re-save — and the map itself — can be
    // removed in consolidation Step 3 without changing stored state.
    let storage = Storage::in_memory(SymmetricKey::generate()).expect("storage");
    let relay = RelayClient::new(
        MockTransport::new(),
        RelayClientConfig {
            transport: TransportConfig::default(),
            max_pending_messages: 100,
            ack_timeout_ms: 30_000,
            max_retries: 3,
            ..Default::default()
        },
        "test-identity".into(),
    );
    let events = Arc::new(EventDispatcher::new());
    let mut controller = SyncController::new(relay, &storage, SyncConfig::default(), events);
    controller.connect(&OsSecureRng::new()).expect("connect");

    let mut snapshots = Vec::new();
    for contact in ["contact-a", "contact-b"] {
        let peer_dh = X3DHKeyPair::generate();
        let ratchet = DoubleRatchetState::initialize_initiator(
            &SymmetricKey::generate(),
            *peer_dh.public_key(),
        )
        .expect("ratchet init");
        let snapshot = serde_json::to_vec(&ratchet.serialize()).expect("snapshot");
        snapshots.push((contact.to_string(), snapshot));
        controller.register_ratchet(contact, ratchet);
    }

    let result = controller.sync(&OsSecureRng::new()).expect("sync runs");
    assert_eq!(result.sent, 0, "no pending updates were queued");

    let after = controller.into_ratchets();
    assert_eq!(after.len(), 2, "both ratchets come back out");
    for (contact, before_bytes) in snapshots {
        let ratchet = after
            .get(&contact)
            .unwrap_or_else(|| panic!("ratchet for {contact} survives the cycle"));
        let after_bytes = serde_json::to_vec(&ratchet.serialize()).expect("serialize after");
        assert_eq!(
            after_bytes, before_bytes,
            "sync must not mutate the ratchet state for {contact}"
        );
    }
}
