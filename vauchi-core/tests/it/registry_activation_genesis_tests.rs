// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 slice 4a — genesis receive persists the carried registry (ADR-064
//! Amendment 2026-07-25, ADR-068 §Decision req 6).
//!
//! The genesis envelope already carries the sender's identity-signed
//! `RegistryBroadcast`; before F4 it was opened, verified, and discarded.
//! Persisting it (guarded, additive) is the cold-start front half: the
//! receiver can afterwards address the orphaned identity's devices, which
//! routing alone never allowed. Both guarded invariants stay untouched —
//! the `[0;32]` re-seat guard and the never-persisted initiator session.

use crate::common;

use common::helpers::create_vauchi_with_identity;
use vauchi_core::SymmetricKey;
use vauchi_core::api::{ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::genesis::GenesisEnvelope;
use vauchi_core::identity::RegistryBroadcast;
use vauchi_core::network::mailbox_token::current_day_epoch;
use vauchi_core::sync::delta::VersionedPayload;
use vauchi_core::sync::registry_activation::ActivationState;
use vauchi_core::sync::safety_alert::{AlertKind, SafetyAlertPayload};

/// Bob holds Alice as an exchanged contact with NO session — the cold-start
/// state after Alice's exchanging device died and a sibling reaches out.
fn cold_start_world() -> (vauchi_core::Vauchi, vauchi_core::Vauchi, String, u64) {
    cold_start_world_ordered(true)
}

/// `alice_initiator` pins the device-pair initiator role (smaller signing
/// key sends first), making the per-device vs genesis-fallback send path
/// deterministic instead of a per-run coin flip.
fn cold_start_world_ordered(
    alice_initiator: bool,
) -> (vauchi_core::Vauchi, vauchi_core::Vauchi, String, u64) {
    let bob_wb = create_vauchi_with_identity("Bob");
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let alice_wb = loop {
        let candidate = create_vauchi_with_identity("Alice");
        let candidate_pk = *candidate.identity().unwrap().signing_public_key();
        if (candidate_pk < bob_pk) == alice_initiator {
            break candidate;
        }
    };

    let alice_pk = *alice_wb.identity().unwrap().signing_public_key();
    let shared = SymmetricKey::generate();
    let alice_contact =
        Contact::from_exchange(alice_pk, ContactCard::new("Alice"), shared.clone(), 0);
    let alice_contact_id = alice_contact.id().to_string();
    bob_wb.add_contact(alice_contact).unwrap();

    let registry_version = alice_wb
        .identity()
        .unwrap()
        .initial_device_registry()
        .version();
    (alice_wb, bob_wb, alice_contact_id, registry_version)
}

fn genesis_alert_blob(
    alice_wb: &vauchi_core::Vauchi,
    bob_pk: &[u8; 32],
    shared: &SymmetricKey,
    nonce: [u8; 32],
) -> Vec<u8> {
    let alice_identity = alice_wb.identity().unwrap();
    let alert = SafetyAlertPayload::new(
        AlertKind::Duress,
        "help".into(),
        1_753_000_000,
        None,
        nonce,
        alice_identity,
        bob_pk,
    )
    .unwrap();
    let payload = VersionedPayload::encode_alert(&alert);
    let broadcast = RegistryBroadcast::new(
        &alice_identity.initial_device_registry(),
        alice_identity.signing_keypair(),
        1_753_000_000,
    );
    let (message, _session) = GenesisEnvelope::seal(
        shared,
        alice_identity,
        bob_pk,
        &broadcast,
        current_day_epoch(1_753_000_000),
        &payload,
    )
    .unwrap();
    serde_json::to_vec(&message).unwrap()
}

// @scenario: multi_device_sync :: A genesis alert seeds the sender's registry for activation
// @internal
#[test]
fn genesis_receive_persists_the_carried_registry_without_activating() {
    let (alice_wb, bob_wb, alice_contact_id, registry_version) = cold_start_world();
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let shared = bob_wb
        .storage()
        .contacts()
        .load_contact(&alice_contact_id)
        .unwrap()
        .unwrap()
        .shared_key()
        .cloned()
        .unwrap();

    let blob = genesis_alert_blob(&alice_wb, &bob_pk, &shared, [4u8; 32]);
    let bob_identity = bob_wb.identity().unwrap();
    let outcome =
        process_single_card_update(bob_identity, bob_wb.storage(), &alice_contact_id, &blob)
            .expect("genesis alert processed");
    assert!(
        matches!(outcome, ReceiveOutcome::Alert(_)),
        "the alert still surfaces exactly as before"
    );

    assert!(
        !bob_wb
            .storage()
            .device()
            .load_contact_active_devices(&alice_contact_id)
            .unwrap()
            .is_empty(),
        "the carried registry is persisted — Bob can now address Alice's devices"
    );
    let tracker = bob_wb
        .storage()
        .registry_activation()
        .load_activation(&alice_contact_id)
        .unwrap()
        .expect("tracker row");
    assert_eq!(tracker.peer_version_held(), Some(registry_version));
    assert_ne!(
        tracker.state(),
        ActivationState::Active,
        "a genesis alert never activates Bob's send side (bilaterality)"
    );

    // Guarded invariant 1 still holds: the cold start persisted the [0;32]
    // responder session (no prior session existed), and no skip was counted.
    assert!(
        bob_wb
            .storage()
            .ratchets()
            .load_ratchet_state_for_device(&alice_contact_id, &[0u8; 32])
            .unwrap()
            .is_some()
    );
    assert_eq!(bob_wb.storage().genesis_limits().reseat_skips().unwrap(), 0);
}

// @scenario: multi_device_sync :: Handshake replies stay on the legacy session before activation
// @internal
#[test]
fn handshake_ack_stays_on_the_legacy_session_before_activation() {
    use vauchi_core::SigningKeyPair;
    use vauchi_core::api::RegistryReplyNeeded;
    use vauchi_core::identity::{DeviceInfo, DeviceRegistry};

    // Bob holds Alice's two-device registry but the handshake is NOT
    // confirmed. In a crossing handshake Alice may hold nothing of Bob's
    // yet, so a per-device ack would be unresolvable on her side and the
    // handshake would strand — the ack must ride the [0;32] session she is
    // guaranteed to resolve. (Pins a design error caught during slice 4b:
    // "the counterparty demonstrably holds our registry" is false when
    // pushes cross.)
    let bob_wb = create_vauchi_with_identity("Bob");
    let alice_identity_kp = (1u32..=100_000)
        .map(|value| {
            let mut seed = [0u8; 32];
            seed[..4].copy_from_slice(&value.to_le_bytes());
            SigningKeyPair::from_seed(&seed)
        })
        .find(|candidate| {
            candidate.public_key().as_bytes() > bob_wb.identity().unwrap().signing_public_key()
        })
        .expect("larger key");
    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange(
        *alice_identity_kp.public_key().as_bytes(),
        ContactCard::new("Alice"),
        shared,
        0,
    );
    let contact_id = contact.id().to_string();
    bob_wb.add_contact(contact).unwrap();

    let device_seed = [77u8; 32];
    let first = DeviceInfo::derive(&device_seed, 0, "A phone".into(), 1);
    let second = DeviceInfo::derive(&device_seed, 1, "A tablet".into(), 1);
    let mut registry = DeviceRegistry::new(first.to_registered(&device_seed), &alice_identity_kp);
    registry
        .add_device(second.to_registered(&device_seed), &alice_identity_kp)
        .unwrap();
    let broadcast = RegistryBroadcast::new(
        &registry,
        &alice_identity_kp,
        bob_wb.storage().clock().unix_seconds(),
    );
    bob_wb
        .storage()
        .device()
        .save_contact_device_registry(
            &contact_id,
            &broadcast,
            alice_identity_kp.public_key().as_bytes(),
            60,
        )
        .unwrap();

    let reply = RegistryReplyNeeded {
        sender_id: contact_id.clone(),
        acked_version: broadcast.version(),
        push_nonce: [6u8; 32],
    };
    bob_wb.queue_registry_ack(&reply).unwrap();

    assert_eq!(
        bob_wb
            .storage()
            .pending()
            .count_pending_updates(&contact_id)
            .unwrap(),
        1,
        "exactly one legacy-session copy — never per-device before Active"
    );
}

// @scenario: multi_device_sync :: A registry-less, session-less device geneses its push
// @internal
#[test]
fn scanner_push_geneses_when_no_session_and_no_registry_exist() {
    // The orphaned-sibling cold start for CARD continuity: no session, no
    // peer registry — the push must ride a genesis envelope the peer can
    // open statelessly, exactly like a duress alert does.
    let (alice_wb, bob_wb, alice_contact_id, _version) = cold_start_world();
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let alice_identity = alice_wb.identity().unwrap();
    let bob_contact = Contact::from_exchange(
        bob_pk,
        ContactCard::new("Bob"),
        bob_wb
            .storage()
            .contacts()
            .load_contact(&alice_contact_id)
            .unwrap()
            .unwrap()
            .shared_key()
            .cloned()
            .unwrap(),
        0,
    );
    let bob_contact_id = bob_contact.id().to_string();
    alice_wb.add_contact(bob_contact).unwrap();

    assert_eq!(alice_wb.queue_registry_pushes().unwrap(), 1);

    let queued = alice_wb
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    assert_eq!(queued.len(), 1);
    let message: vauchi_core::crypto::ratchet::RatchetMessage =
        serde_json::from_slice(&queued[0].payload).expect("wire-ordinary ratchet message");
    let shared = alice_wb
        .storage()
        .contacts()
        .load_contact(&bob_contact_id)
        .unwrap()
        .unwrap()
        .shared_key()
        .cloned()
        .unwrap();
    let opened = GenesisEnvelope::open(
        &shared,
        alice_identity.signing_public_key(),
        &bob_pk,
        &message,
    )
    .expect("statelessly openable genesis envelope");
    assert!(
        matches!(
            VersionedPayload::decode(&opened.inner_payload),
            Ok(VersionedPayload::RegistryPush(_))
        ),
        "the genesis envelope carries the registry push"
    );
}

// @scenario: multi_device_sync :: An orphaned sibling re-converges to Active without re-exchange
// @internal
#[test]
fn orphaned_sibling_reconverges_and_receives_cards_without_reexchange() {
    // allow(zero_assertions): ordering-parameterized wrapper — every
    // assertion lives in run_unorphaning_flow.
    run_unorphaning_flow(true);
}

// @scenario: multi_device_sync :: Un-orphaning converges even when the sibling is the session responder
// @internal
#[test]
fn orphaned_responder_sibling_reconverges_via_genesis_fallback() {
    // allow(zero_assertions): ordering-parameterized wrapper — every
    // assertion lives in run_unorphaning_flow.
    //
    // The device-pair initiator role is decided by key order. When A2
    // lands on the responder side, her fresh per-device session cannot
    // send first — the Active ack path must fall back to the genesis
    // envelope or the handshake livelocks (A2's scanner skips Active
    // contacts, so nothing would ever retry). Caught as an intermittent
    // failure of the initiator-ordering test.
    run_unorphaning_flow(false);
}

fn run_unorphaning_flow(alice_initiator: bool) {
    // The lost-primary repair, end to end. Alice's exchanging device is
    // gone: her sibling A2 holds only the synced shared_key — no session,
    // no peer registry. Bob still holds a [0;32] session anchored to the
    // dead device.
    let (alice_wb, bob_wb, alice_contact_id, _v) = cold_start_world_ordered(alice_initiator);
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let shared = bob_wb
        .storage()
        .contacts()
        .load_contact(&alice_contact_id)
        .unwrap()
        .unwrap()
        .shared_key()
        .cloned()
        .unwrap();
    let bob_contact = Contact::from_exchange(bob_pk, ContactCard::new("Bob"), shared, 0);
    let bob_contact_id = bob_contact.id().to_string();
    alice_wb.add_contact(bob_contact).unwrap();
    // Bob's [0;32] session is anchored to the DEAD device — A2 cannot use it.
    {
        use vauchi_core::crypto::ratchet::DoubleRatchetState;
        use vauchi_core::exchange::X3DHKeyPair;
        let dead_dh = X3DHKeyPair::generate();
        let dead_shared = bob_wb
            .storage()
            .contacts()
            .load_contact(&alice_contact_id)
            .unwrap()
            .unwrap()
            .shared_key()
            .cloned()
            .unwrap();
        let bob_ratchet =
            DoubleRatchetState::initialize_initiator(&dead_shared, *dead_dh.public_key()).unwrap();
        bob_wb
            .storage()
            .ratchets()
            .save_ratchet_state(&alice_contact_id, &bob_ratchet, true)
            .unwrap();
    }

    let alice_identity = alice_wb.identity().unwrap();
    let bob_identity = bob_wb.identity().unwrap();

    // 1. A2's sync tick geneses a registry push.
    assert_eq!(alice_wb.queue_registry_pushes().unwrap(), 1);
    let push_blob = alice_wb
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap()
        .remove(0)
        .payload;

    // 2. Bob receives it — his [0;32] decrypt fails, the genesis fallback
    //    opens it, and Alice's registry is persisted.
    let reply = match process_single_card_update(
        bob_identity,
        bob_wb.storage(),
        &alice_contact_id,
        &push_blob,
    )
    .expect("bob opens the genesis push")
    {
        ReceiveOutcome::RegistryPushReceived(reply) => reply,
        other => panic!("expected RegistryPushReceived, got {other:?}"),
    };

    // 3. Bob acks — pre-activation, so the ack is genesis-sealed too.
    bob_wb.queue_registry_ack(&reply).unwrap();
    let ack_blob = bob_wb
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap()
        .remove(0)
        .payload;

    // 4. A2 opens the ack statelessly, activates, and must confirm back.
    let confirm = match process_single_card_update(
        alice_identity,
        alice_wb.storage(),
        &bob_contact_id,
        &ack_blob,
    )
    .expect("A2 opens the genesis ack")
    {
        ReceiveOutcome::RegistryAckReceived { reply, .. } => {
            reply.expect("activation transition requires a confirming reply")
        }
        other => panic!("expected RegistryAckReceived, got {other:?}"),
    };
    let a2_tracker = alice_wb
        .storage()
        .registry_activation()
        .load_activation(&bob_contact_id)
        .unwrap()
        .expect("tracker");
    assert_eq!(a2_tracker.state(), ActivationState::Active, "A2 activates");

    // 5. A2's confirm reaches Bob. As device-pair initiator it rides the
    //    ordinary per-device path; as responder the fresh pair cannot send
    //    first and the confirm falls back to the genesis envelope.
    alice_wb.queue_registry_ack(&confirm).unwrap();
    let confirm_blobs = alice_wb
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    let confirm_blob = confirm_blobs
        .iter()
        .next_back()
        .map(|u| u.payload.clone())
        .expect("confirm queued");
    if alice_initiator {
        // In production Bob resolves A2's device from her rotating sender
        // token (he holds Alice's registry now); the device-aware entry
        // mirrors that.
        let a2_device_id = *alice_identity.device_id();
        vauchi_core::api::process_single_card_update_for_device(
            bob_identity,
            bob_wb.storage(),
            &alice_contact_id,
            &a2_device_id,
            &confirm_blob,
        )
        .expect("bob processes the per-device confirm");
    } else {
        // The genesis-sealed confirm opens on Bob's plain [0;32] path via
        // the decrypt-failure fallback.
        process_single_card_update(
            bob_identity,
            bob_wb.storage(),
            &alice_contact_id,
            &confirm_blob,
        )
        .expect("bob processes the genesis confirm");
    }
    let bob_tracker = bob_wb
        .storage()
        .registry_activation()
        .load_activation(&alice_contact_id)
        .unwrap()
        .expect("tracker");
    assert_eq!(
        bob_tracker.state(),
        ActivationState::Active,
        "both sides active — the relationship is un-orphaned"
    );

    // 6. The payoff: Bob's next card update fans out per-device, and A2
    //    can decrypt her copy — card continuity without re-exchange.
    let bob_own_field =
        vauchi_core::ContactField::new(vauchi_core::FieldType::Email, "Work", "bob@example.com", 0);
    let field_id = bob_own_field.id().to_string();
    bob_wb.add_own_field(bob_own_field).unwrap();
    bob_wb.set_own_field_public(&field_id).unwrap();
    let empty = ContactCard::new("Bob");
    let current = bob_wb
        .storage()
        .contacts()
        .load_own_card()
        .unwrap()
        .unwrap();
    let updates = bob_wb
        .prepare_card_updates_for_contact(&alice_contact_id, &empty, &current)
        .unwrap();
    assert!(
        updates.iter().all(|(device_id, _)| *device_id != [0u8; 32]),
        "cards now ride per-device sessions, not the dead legacy chain"
    );
    let (_device_id, card_blob) = updates.into_iter().next().expect("one per-device copy");
    let bob_device_id = *bob_identity.device_id();
    let outcome = vauchi_core::api::process_single_card_update_for_device(
        alice_identity,
        alice_wb.storage(),
        &bob_contact_id,
        &bob_device_id,
        &card_blob,
    )
    .expect("A2 decrypts the card update her sibling's death used to orphan");
    assert!(matches!(outcome, ReceiveOutcome::CardDelta));
}

// @scenario: multi_device_sync :: A stale carried registry never clobbers a newer held one
// @internal
#[test]
fn genesis_receive_tolerates_an_already_newer_held_registry() {
    let (alice_wb, bob_wb, alice_contact_id, registry_version) = cold_start_world();
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let alice_identity = alice_wb.identity().unwrap();
    let shared = bob_wb
        .storage()
        .contacts()
        .load_contact(&alice_contact_id)
        .unwrap()
        .unwrap()
        .shared_key()
        .cloned()
        .unwrap();

    // Bob already holds Alice's registry (e.g. via an earlier vouched push).
    let held = RegistryBroadcast::new(
        &alice_identity.initial_device_registry(),
        alice_identity.signing_keypair(),
        1_753_000_100,
    );
    bob_wb
        .storage()
        .device()
        .save_contact_device_registry(
            &alice_contact_id,
            &held,
            alice_identity.signing_public_key(),
            u64::MAX,
        )
        .unwrap();

    let blob = genesis_alert_blob(&alice_wb, &bob_pk, &shared, [5u8; 32]);
    let bob_identity = bob_wb.identity().unwrap();
    let outcome =
        process_single_card_update(bob_identity, bob_wb.storage(), &alice_contact_id, &blob)
            .expect("same-version carried registry is an idempotent no-op");
    assert!(matches!(outcome, ReceiveOutcome::Alert(_)));

    let tracker = bob_wb
        .storage()
        .registry_activation()
        .load_activation(&alice_contact_id)
        .unwrap()
        .expect("tracker row");
    assert_eq!(tracker.peer_version_held(), Some(registry_version));
}
