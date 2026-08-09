// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! RED characterization: a coercion-safety / duress alert raised on a SECONDARY
//! linked device (one that joined via device-link and never did the QR exchange)
//! is silently dropped at send.
//!
//! Context — `backlog/2026-07-21-per-device-ratchet-registry-dormant` and
//! `problems/2026-07-10-multi-device-ratchet-topology-gap`. Per-device ratchet
//! sessions are keyed `(contact_id, peer_device_id)`, bootstrapped from a peer
//! `RegistryBroadcast`. That registry is persisted ONLY by the device-link path
//! (`DeviceSyncOrchestrator::apply_full_sync` -> `save_contact_device_registry`),
//! never from a contact exchange, and ratchet state itself is never synced
//! between an owner's devices (`api/sync/device_orchestrator.rs:388-390` "never a
//! live ratchet chain"). So a secondary device ends up holding the exchanged
//! Contact (with `shared_key` + `public_key`, copied by `DeviceSyncPayload`) but
//! NO ratchet row and NO peer registry for that contact.
//!
//! Card updates survive this via owner-device repropagation + idempotent LWW (a
//! secondary's edit syncs to the exchanged device, which repropagates it). A
//! one-shot safety/duress alert has no such repropagation path: it is built
//! per-recipient by `queue_safety_alerts` (`api/vauchi/emergency.rs:121`) through
//! `encrypt_payload_for_contact_devices`, which — with an empty registry and no
//! `[0;32]` ratchet — returns `Err(NotFound("ratchet state"))`
//! (`api/vauchi/propagation.rs:350`); `queue_safety_alerts` catches that as
//! `=> continue` (`emergency.rs:167`) and silently skips the recipient. Via
//! `authenticate()` the duress path additionally swallows the count
//! (`api/vauchi/security.rs:122-129`), so the coerced user gets zero signal the
//! alarm never sent.
//!
//! Two tests are controls: an alert from a device WITH a ratchet is delivered
//! and received; a real owner-sync leaves the secondary device without a ratchet
//! or registry. The remaining tests were the RED spec for this gap and are now
//! GREEN regression tests: the ADR-068 genesis envelope (MR B) closes it — the
//! send path bootstraps a session from the synced `shared_key`, and the receiver
//! opens the genesis envelope on the `[0;32]` failure path. See
//! `exchange::genesis` and `backlog/2026-07-21-per-device-ratchet-registry-dormant`.

use crate::common;

use common::device_sync::{
    create_test_contact, create_test_device, create_test_registry, create_test_storage,
};
use common::helpers::{create_vauchi_with_identity, setup_alice_bob_exchange, setup_ratchets};
use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::api::sync::card_update::process_single_card_update_for_authenticated_device;
use vauchi_core::api::{CardUpdateError, ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::ratchet::{DoubleRatchetState, RatchetMessage};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::exchange::genesis::GenesisEnvelope;
use vauchi_core::identity::{DeviceRegistry, Identity, RegistryBroadcast};
use vauchi_core::storage::GENESIS_CONTACT_ATTEMPTS_PER_WINDOW;
use vauchi_core::sync::DeviceLinkIntent;
use vauchi_core::sync::delta::VersionedPayload;
use vauchi_core::sync::registry_activation::ActivationTracker;
use vauchi_core::sync::safety_alert::{AlertKind, SafetyAlertPayload};
use vauchi_core::types::DuressSettings;
use vauchi_core::{AuthMode, SymmetricKey, Vauchi};

/// A secondary linked device's verified state after device-link + owner-device
/// sync: it holds the exchanged `Bob` contact (shared_key + public_key) but has
/// NO ratchet row and NO peer registry for Bob. Reproduced directly here; the
/// `real_owner_sync_leaves_secondary_device_without_ratchet_or_registry` control
/// below proves a genuine `DeviceSyncOrchestrator` sync produces exactly this
/// state. Returns `(secondary_device, bob_contact_id)`.
fn secondary_device_after_owner_sync() -> (Vauchi, String) {
    let secondary = create_vauchi_with_identity("Alice");
    let bob_pk = *create_vauchi_with_identity("Bob")
        .identity()
        .unwrap()
        .signing_public_key();
    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), SymmetricKey::generate(), 0);
    let bob_id = bob_contact.id().to_string();
    secondary.add_contact(bob_contact).unwrap();
    // Deliberately NO save_ratchet_state and NO save_contact_device_registry —
    // this is the exact end-state of a secondary device (see module docs).
    (secondary, bob_id)
}

// ---------------------------------------------------------------------------
// Controls (pass) — the harness works and the premise holds.
// ---------------------------------------------------------------------------

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// Control: an emergency alert from a device that DOES hold a ratchet is queued
/// and received. Isolates the secondary-device failure below as the real gap.
// @internal
#[test]
fn emergency_alert_with_established_ratchet_reaches_recipient() {
    let (mut alice_wb, bob_wb, secret, bob_id_at_alice, alice_id_at_bob) =
        setup_alice_bob_exchange();
    // Alice (initiator) is the sender; Bob (responder) receives — matching the
    // pairing setup_ratchets returns.
    let (alice_ratchet, bob_ratchet) = setup_ratchets(&secret);
    alice_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&bob_id_at_alice, &alice_ratchet, true)
        .unwrap();
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&alice_id_at_bob, &bob_ratchet, false)
        .unwrap();

    alice_wb
        .configure_emergency_broadcast(vec![bob_id_at_alice.clone()], "check on me".into(), false)
        .unwrap();
    let result = alice_wb.send_emergency_broadcast().unwrap();
    assert_eq!(
        result.sent, 1,
        "a device with a ratchet must deliver the alert"
    );

    let pending = alice_wb
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    let blob = &pending
        .iter()
        .find(|u| u.contact_id == bob_id_at_alice)
        .expect("a queued alert for Bob")
        .payload;
    let outcome = process_single_card_update(
        bob_wb.identity().unwrap(),
        bob_wb.storage(),
        &alice_id_at_bob,
        blob,
    )
    .expect("the alert must be received");
    match outcome {
        ReceiveOutcome::Alert(a) => assert_eq!(a.kind, AlertKind::Emergency),
        other => panic!("expected an Emergency alert, got {other:?}"),
    }
}

/// Control (premise): a genuine `DeviceSyncOrchestrator` full-sync from an
/// exchanged device (A1) to a freshly linked device (A2) leaves A2 with the Bob
/// contact but ZERO ratchet rows and ZERO peer-registry rows for Bob. This is
/// the state the RED tests below rely on — proven here through the real module,
/// not assumed.
// @internal
#[test]
fn real_owner_sync_leaves_secondary_device_without_ratchet_or_registry() {
    let master_seed = [0x42u8; 32];

    // A1: exchanged with Bob — has the contact and a [0;32] session.
    let a1 = create_test_storage();
    let bob = create_test_contact("Bob");
    let bob_id = bob.id().to_string();
    a1.contacts().save_contact(&bob).unwrap();
    let dh = X3DHKeyPair::generate();
    let a1_ratchet =
        DoubleRatchetState::initialize_initiator(&SymmetricKey::generate(), *dh.public_key())
            .unwrap();
    a1.ratchets()
        .save_ratchet_state(&bob_id, &a1_ratchet, true)
        .unwrap();

    let a1_device = create_test_device(&master_seed, 0, "Device A");
    let a1_registry = create_test_registry(&master_seed, &a1_device);
    let payload = DeviceSyncOrchestrator::new(&a1, a1_device, a1_registry)
        .create_full_sync_payload(DeviceLinkIntent::AddDevice)
        .unwrap();
    assert!(
        payload.contact_device_registries.is_empty(),
        "a single-device peer has no registry to propagate"
    );

    // A2: freshly linked — applies A1's full sync.
    let a2 = create_test_storage();
    let a2_device = create_test_device(&master_seed, 1, "Device B");
    let a2_registry = create_test_registry(&master_seed, &a2_device);
    DeviceSyncOrchestrator::new(&a2, a2_device, a2_registry)
        .apply_full_sync(payload)
        .unwrap();

    assert!(
        a2.contacts().load_contact(&bob_id).unwrap().is_some(),
        "A2 must receive the Bob contact via owner-device sync"
    );
    assert!(
        a2.ratchets().load_ratchet_state(&bob_id).unwrap().is_none(),
        "ratchet state is never synced — A2 has no [0;32] session for Bob"
    );
    assert!(
        a2.device()
            .load_contact_active_devices(&bob_id)
            .unwrap()
            .is_empty(),
        "no peer registry is persisted for an exchanged single-device contact"
    );
}

// ---------------------------------------------------------------------------
// Regression — the safe behavior, now delivered by the ADR-068 genesis path.
// ---------------------------------------------------------------------------

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// A secondary device must be able to raise an emergency alert to an exchanged
/// contact. Before genesis the send returned `sent == 0` (silently skipped);
/// the shared-key-rooted genesis envelope now queues it.
// @internal
#[test]
fn emergency_alert_from_secondary_device_should_reach_recipient() {
    let (secondary, bob_id) = secondary_device_after_owner_sync();
    let mut secondary = secondary;

    secondary
        .configure_emergency_broadcast(vec![bob_id.clone()], "check on me".into(), false)
        .unwrap();
    let result = secondary.send_emergency_broadcast().unwrap();

    assert_eq!(
        result.sent, 1,
        "a secondary linked device must be able to raise an emergency alert"
    );
    let pending = secondary
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    assert!(
        pending.iter().any(|u| u.contact_id == bob_id),
        "the emergency alert must be queued for the contact"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// Most safety-critical: a duress unlock on a secondary device must queue the
/// disguised alert to the configured contact. Before genesis the unlock
/// succeeded (`AuthMode::Duress`) while silently queuing nothing — the coerced
/// user got no signal the alarm failed. Genesis now queues it.
// @internal
#[test]
fn duress_unlock_from_secondary_device_should_queue_alert() {
    let (secondary, bob_id) = secondary_device_after_owner_sync();
    let mut secondary = secondary;

    secondary.setup_app_password("normal-pin-1234").unwrap();
    secondary.setup_duress_password("987654").unwrap();
    secondary
        .save_duress_settings(&DuressSettings {
            alert_contact_ids: vec![bob_id.clone()],
            alert_message: "I need help".to_string(),
            include_location: false,
        })
        .unwrap();

    let mode = secondary
        .authenticate("987654")
        .expect("duress unlock must succeed");
    assert_eq!(mode, AuthMode::Duress);

    let pending = secondary
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    assert!(
        pending.iter().any(|u| u.contact_id == bob_id),
        "the duress unlock must queue a disguised alert for the configured contact"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// End-to-end: a secondary device with only `shared_key` seals a genesis
/// alert; the recipient (holding the matching contact and shared_key, no
/// session) opens it, surfaces the emergency, and persists a durable fact.
/// This is the full ADR-068 send→receive path, not just sender pending state.
// @internal
#[test]
fn secondary_device_alert_should_be_surfaced_by_recipient() {
    // Alice's secondary device and Bob's device share one relationship key but
    // neither holds a ratchet or peer registry — the exact secondary-device
    // end-state (see `secondary_device_after_owner_sync`), mirrored on both
    // sides so a real receive can run.
    let mut alice_secondary = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let alice_pk = *alice_secondary.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let shared_bytes = [0x5au8; 32];

    let bob_contact_at_alice = Contact::from_exchange(
        bob_pk,
        ContactCard::new("Bob"),
        SymmetricKey::from_bytes(shared_bytes),
        0,
    );
    let bob_id_at_alice = bob_contact_at_alice.id().to_string();
    alice_secondary.add_contact(bob_contact_at_alice).unwrap();

    let alice_contact_at_bob = Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        SymmetricKey::from_bytes(shared_bytes),
        0,
    );
    let alice_id_at_bob = alice_contact_at_bob.id().to_string();
    bob.add_contact(alice_contact_at_bob).unwrap();

    // Alice raises the alarm from the secondary device.
    alice_secondary
        .configure_emergency_broadcast(vec![bob_id_at_alice.clone()], "check on me".into(), false)
        .unwrap();
    let result = alice_secondary.send_emergency_broadcast().unwrap();
    assert_eq!(result.sent, 1, "the secondary device must queue the alert");

    let pending = alice_secondary
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    let blob = &pending
        .iter()
        .find(|u| u.contact_id == bob_id_at_alice)
        .expect("a queued genesis alert for Bob")
        .payload;

    // Bob receives the genesis blob with no prior session for Alice.
    let outcome = process_single_card_update(
        bob.identity().unwrap(),
        bob.storage(),
        &alice_id_at_bob,
        blob,
    )
    .expect("the genesis alert must be received");
    match outcome {
        ReceiveOutcome::Alert(a) => {
            assert_eq!(a.kind, AlertKind::Emergency);
            assert_eq!(a.message, "check on me");
        }
        other => panic!("expected an Emergency alert, got {other:?}"),
    }

    // The alert is durably recorded so it survives a crash before surfacing.
    let facts = bob
        .storage()
        .safety_alerts()
        .load_unsurfaced_facts()
        .unwrap();
    assert_eq!(
        facts.len(),
        1,
        "the received genesis alert must persist as a durable fact"
    );
    assert_eq!(facts[0].contact_id, alice_id_at_bob);

    // A replay of the same blob must not create a second alert or fact.
    let replay = process_single_card_update(
        bob.identity().unwrap(),
        bob.storage(),
        &alice_id_at_bob,
        blob,
    );
    assert!(replay.is_err(), "replayed genesis blob must be rejected");
    assert_eq!(
        bob.storage()
            .safety_alerts()
            .load_unsurfaced_facts()
            .unwrap()
            .len(),
        1,
        "a replay must not create a second durable fact"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// After registry activation, a sender that is the deterministic responder for
/// a peer-device ratchet has no sending chain until that peer sends first.
/// Safety alerts cannot wait for ordinary traffic: they must use the same
/// device-scoped, non-persistent genesis fallback as responder-side card
/// deltas.
// @internal
#[test]
fn active_responder_device_genesis_seals_emergency_alert() {
    let sender_seed = [0xa5u8; 32];
    let sender_identity =
        Identity::from_device_link(sender_seed, "Alice".into(), 0, "Alice phone".into(), 1);
    let sender_pk = *sender_identity.signing_public_key();

    let (recipient_seed, recipient_identity) = (1u32..=100_000)
        .find_map(|value| {
            let mut seed = [0u8; 32];
            seed[..4].copy_from_slice(&value.to_le_bytes());
            let identity = Identity::from_device_link(seed, "Bob".into(), 0, "Bob phone".into(), 1);
            (identity.signing_public_key() < &sender_pk).then_some((seed, identity))
        })
        .expect("a lexicographically smaller recipient identity");
    let recipient_pk = *recipient_identity.signing_public_key();
    let recipient_device_id = *recipient_identity.device_id();
    assert!(
        sender_pk > recipient_pk,
        "the sender must bootstrap as the responder for this regression"
    );

    let shared = SymmetricKey::from_bytes([0x5au8; 32]);
    let mut sender = Vauchi::in_memory().unwrap();
    sender.set_identity(sender_identity).unwrap();
    let recipient_contact =
        Contact::from_exchange(recipient_pk, ContactCard::new("Bob"), shared.clone(), 0);
    let recipient_id = recipient_contact.id().to_string();
    sender.add_contact(recipient_contact).unwrap();

    let recipient_registry = DeviceRegistry::new(
        recipient_identity
            .device_info()
            .to_registered(&recipient_seed),
        recipient_identity.signing_keypair(),
    );
    let recipient_broadcast = RegistryBroadcast::new(
        &recipient_registry,
        recipient_identity.signing_keypair(),
        sender.storage().clock().unix_seconds(),
    );
    sender
        .storage()
        .device()
        .save_contact_device_registry(&recipient_id, &recipient_broadcast, &recipient_pk, 60)
        .unwrap();
    let mut activation = ActivationTracker::new();
    activation.record_push_sent([7u8; 32], 1);
    activation.record_ack(&[7u8; 32], 1).unwrap();
    sender
        .storage()
        .registry_activation()
        .save_activation(&recipient_id, &activation)
        .unwrap();

    sender
        .configure_emergency_broadcast(
            vec![recipient_id.clone()],
            "active responder alert".into(),
            false,
        )
        .unwrap();
    let result = sender.send_emergency_broadcast().unwrap();
    assert_eq!(
        result.sent, 1,
        "an Active responder must genesis-seal instead of dropping the alert"
    );

    let pending = sender
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    assert_eq!(pending.len(), 1, "one peer device gets one alert copy");
    assert_eq!(
        pending[0].target_device_id,
        Some(recipient_device_id),
        "the fallback must stay on the peer device's mailbox"
    );
    assert!(
        sender
            .storage()
            .ratchets()
            .load_ratchet_state_for_device(&recipient_id, &recipient_device_id)
            .unwrap()
            .is_none(),
        "a genesis-born sending session must not be persisted"
    );

    let sender_device_id = *sender.identity().unwrap().device_id();
    let mut recipient = Vauchi::in_memory().unwrap();
    recipient.set_identity(recipient_identity).unwrap();
    let sender_contact = Contact::from_exchange(sender_pk, ContactCard::new("Alice"), shared, 0);
    let sender_id = sender_contact.id().to_string();
    recipient.add_contact(sender_contact).unwrap();
    let sender_registry = DeviceRegistry::new(
        sender
            .identity()
            .unwrap()
            .device_info()
            .to_registered(&sender_seed),
        sender.identity().unwrap().signing_keypair(),
    );
    let sender_broadcast = RegistryBroadcast::new(
        &sender_registry,
        sender.identity().unwrap().signing_keypair(),
        recipient.storage().clock().unix_seconds(),
    );
    recipient
        .storage()
        .device()
        .save_contact_device_registry(&sender_id, &sender_broadcast, &sender_pk, 60)
        .unwrap();

    let outcome = process_single_card_update_for_authenticated_device(
        recipient.identity().unwrap(),
        recipient.storage(),
        &sender_id,
        &sender_device_id,
        &pending[0].payload,
    )
    .expect("the peer device must open the genesis-sealed alert");
    assert!(
        matches!(
            outcome,
            ReceiveOutcome::Alert(ref alert)
                if alert.kind == AlertKind::Emergency
                    && alert.message == "active responder alert"
        ),
        "the received payload must be the original emergency alert"
    );
    assert!(
        process_single_card_update_for_authenticated_device(
            recipient.identity().unwrap(),
            recipient.storage(),
            &sender_id,
            &sender_device_id,
            &pending[0].payload,
        )
        .is_err(),
        "replaying the genesis alert must be rejected"
    );
}

/// A well-formed `RatchetMessage` that is not a genesis envelope — it fails the
/// genesis open (AEAD mismatch under the shared-key-derived responder), so it
/// consumes one genesis-decrypt budget unit and then falls back.
fn non_genesis_ratchet_blob(seed: u8) -> Vec<u8> {
    let message = RatchetMessage {
        dh_public: [seed; 32],
        dh_generation: 0,
        message_index: 0,
        previous_chain_length: 0,
        ciphertext: vec![seed; 96],
    };
    serde_json::to_vec(&message).unwrap()
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// A session-less contact's genesis-decrypt budget can be exhausted; once it is,
/// a further attempt returns the retriable `GenesisRateLimited` (which the
/// receive loop must not ACK), so a burst of undecryptable traffic delays but
/// never silently drops a legitimate first-contact alert (plan §REVISION F6).
// @internal
#[test]
fn genesis_decrypt_budget_exhaustion_is_retriable_not_dropped() {
    let bob = create_vauchi_with_identity("Bob");
    let alice_contact = Contact::from_exchange(
        [0x11u8; 32],
        ContactCard::new("Alice"),
        SymmetricKey::from_bytes([0x22u8; 32]),
        0,
    );
    let alice_id = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    // Drive the per-contact budget to exactly its cap with non-genesis blobs;
    // each consumes one unit on the no-session arm and then fails closed.
    for i in 0..GENESIS_CONTACT_ATTEMPTS_PER_WINDOW {
        let blob = non_genesis_ratchet_blob(i as u8);
        let err =
            process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &blob)
                .expect_err("a non-genesis blob must not be accepted");
        assert!(
            matches!(err, CardUpdateError::NoRatchetState),
            "within budget, a non-genesis blob falls back to the ordinary error, got {err:?}"
        );
    }

    // The next attempt is denied by the exhausted budget — retriable, distinct.
    let blob = non_genesis_ratchet_blob(0xff);
    let err = process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &blob)
        .expect_err("the over-budget attempt must be denied");
    assert!(
        matches!(err, CardUpdateError::GenesisRateLimited),
        "an exhausted budget must surface as retriable GenesisRateLimited, got {err:?}"
    );
}

/// Builds a session-less Bob holding a contact for `alice`, sharing `shared`.
/// Returns `(bob, alice_contact_id, alice_broadcast)`.
fn session_less_pair(
    alice: &Identity,
    shared_bytes: [u8; 32],
) -> (Vauchi, String, RegistryBroadcast) {
    let bob = create_vauchi_with_identity("Bob");
    let alice_contact = Contact::from_exchange(
        *alice.signing_public_key(),
        ContactCard::new("Alice"),
        SymmetricKey::from_bytes(shared_bytes),
        0,
    );
    let alice_id = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();
    let broadcast =
        RegistryBroadcast::new(&alice.initial_device_registry(), alice.signing_keypair(), 0);
    (bob, alice_id, broadcast)
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// A genesis envelope that decrypts and verifies but wraps a NON-alert inner
/// payload must be rejected — `shared_key` admits the blob to the parser, it
/// does not authorize an arbitrary payload (plan §REVISION F8). No fact is
/// created.
// @internal
#[test]
fn genesis_wrapping_a_non_alert_payload_is_rejected() {
    let alice = Identity::create("Alice", 0);
    let shared_bytes = [0x33u8; 32];
    let (bob, alice_id, broadcast) = session_less_pair(&alice, shared_bytes);

    // Seal an inner payload that is not a `VersionedPayload::Alert` (0x04).
    let (message, _) = GenesisEnvelope::seal(
        &SymmetricKey::from_bytes(shared_bytes),
        &alice,
        bob.identity().unwrap().signing_public_key(),
        &broadcast,
        20_100,
        b"\x99 not a known payload",
    )
    .expect("seal");
    let blob = serde_json::to_vec(&message).unwrap();

    let err = process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &blob)
        .expect_err("a genesis envelope wrapping a non-alert payload must be rejected");
    assert!(
        matches!(err, CardUpdateError::NoRatchetState),
        "a non-alert genesis payload falls back to the ordinary error, got {err:?}"
    );
    assert!(
        bob.storage()
            .safety_alerts()
            .load_unsurfaced_facts()
            .unwrap()
            .is_empty(),
        "a rejected genesis must persist no durable fact"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// A valid genesis envelope (envelope signature checks out) whose INNER alert
/// carries an invalid sender signature must be rejected: possession of
/// `shared_key` is admission to the parser, never authority over the alert
/// content (plan §REVISION F8). The inner alert here is signed by a third party,
/// not the contact.
// @internal
#[test]
fn genesis_with_a_forged_inner_alert_signature_is_rejected() {
    let alice = Identity::create("Alice", 0);
    let mallory = Identity::create("Mallory", 0);
    let shared_bytes = [0x44u8; 32];
    let (bob, alice_id, broadcast) = session_less_pair(&alice, shared_bytes);
    let bob_pk = *bob.identity().unwrap().signing_public_key();

    // The inner alert is signed by Mallory, so it cannot verify against Alice's
    // identity — but the OUTER envelope is validly signed by Alice.
    let forged = SafetyAlertPayload::new(
        AlertKind::Emergency,
        "I need help".to_string(),
        7_000,
        None,
        [7u8; 32],
        &mallory,
        &bob_pk,
    )
    .expect("alert construction");
    let inner = VersionedPayload::encode_alert(&forged);

    let (message, _) = GenesisEnvelope::seal(
        &SymmetricKey::from_bytes(shared_bytes),
        &alice,
        &bob_pk,
        &broadcast,
        20_100,
        &inner,
    )
    .expect("seal");
    let blob = serde_json::to_vec(&message).unwrap();

    let err = process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &blob)
        .expect_err("a forged inner alert signature must be rejected");
    assert!(
        matches!(err, CardUpdateError::SignatureInvalid),
        "a forged inner alert signature must fail closed, got {err:?}"
    );
    assert!(
        bob.storage()
            .safety_alerts()
            .load_unsurfaced_facts()
            .unwrap()
            .is_empty(),
        "a forged alert must persist no durable fact"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// On the decrypt-failure arm (a `[0;32]` session already exists), a
/// rate-limited genesis attempt must fall through to the ordinary
/// `DecryptionFailed` (which is ACKed) rather than the retained
/// `GenesisRateLimited`, so ordinary undecryptable traffic over an established
/// session cannot pin blobs on the relay (plan §REVISION C2/F6).
// @internal
#[test]
fn established_session_decrypt_failure_falls_through_when_rate_limited() {
    let bob = create_vauchi_with_identity("Bob");
    let alice_contact = Contact::from_exchange(
        [0x11u8; 32],
        ContactCard::new("Alice"),
        SymmetricKey::from_bytes([0x55u8; 32]),
        0,
    );
    let alice_id = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    // Give Bob an established legacy [0;32] session so garbage blobs reach the
    // decrypt-failure arm, not the no-session arm.
    let session =
        DoubleRatchetState::initialize_initiator(&SymmetricKey::generate(), [0x66u8; 32]).unwrap();
    bob.storage()
        .ratchets()
        .save_ratchet_state_for_device(&alice_id, &[0; 32], &session, true)
        .unwrap();

    // Exhaust the per-contact budget with undecryptable blobs; each fails the
    // session decrypt, then the speculative genesis attempt.
    for i in 0..GENESIS_CONTACT_ATTEMPTS_PER_WINDOW {
        let blob = non_genesis_ratchet_blob(i as u8);
        let err =
            process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &blob)
                .expect_err("an undecryptable blob must not be accepted");
        assert!(
            matches!(err, CardUpdateError::DecryptionFailed),
            "got {err:?}"
        );
    }

    // Budget is now exhausted. A further undecryptable blob must STILL surface
    // as the ACKable DecryptionFailed — not the retained GenesisRateLimited.
    let blob = non_genesis_ratchet_blob(0xfe);
    let err = process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &blob)
        .expect_err("the over-budget blob must be denied");
    assert!(
        matches!(err, CardUpdateError::DecryptionFailed),
        "an exhausted budget on the established-session arm must fall through to \
         DecryptionFailed (ACKed), not GenesisRateLimited (retained), got {err:?}"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// A malformed (non-`RatchetMessage`) blob to a session-less contact is rejected
/// at the genesis parse boundary before any key derivation or budget charge —
/// it falls back to the ordinary error and persists nothing.
// @internal
#[test]
fn malformed_blob_to_session_less_contact_falls_back_without_charging_budget() {
    let alice = Identity::create("Alice", 0);
    let (bob, alice_id, _broadcast) = session_less_pair(&alice, [0x77u8; 32]);

    let err = process_single_card_update(
        bob.identity().unwrap(),
        bob.storage(),
        &alice_id,
        b"this is not a ratchet message",
    )
    .expect_err("a malformed blob must be rejected");
    assert!(
        matches!(err, CardUpdateError::NoRatchetState),
        "a malformed genesis candidate falls back to the ordinary error, got {err:?}"
    );

    // Parse failure precedes the budget charge, so a full budget remains.
    assert_eq!(
        bob.storage()
            .genesis_limits()
            .contact_attempts_in_window(&alice_id)
            .unwrap(),
        0,
        "a blob rejected at the parse boundary must not consume budget"
    );
}

/// A genuine genesis alert blob from `alice` to `bob_pk` under `shared_bytes`,
/// carrying the given signed `nonce`.
fn genesis_alert_blob(
    alice: &Identity,
    bob_pk: &[u8; 32],
    shared_bytes: [u8; 32],
    broadcast: &RegistryBroadcast,
    nonce: [u8; 32],
) -> Vec<u8> {
    let alert = SafetyAlertPayload::new(
        AlertKind::Emergency,
        "check on me".to_string(),
        7_000,
        None,
        nonce,
        alice,
        bob_pk,
    )
    .expect("alert construction");
    let inner = VersionedPayload::encode_alert(&alert);
    let (message, _) = GenesisEnvelope::seal(
        &SymmetricKey::from_bytes(shared_bytes),
        alice,
        bob_pk,
        broadcast,
        20_100,
        &inner,
    )
    .expect("seal");
    serde_json::to_vec(&message).unwrap()
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// A COMMIT failure while accepting a genesis alert must roll back cleanly:
/// nothing is persisted, the blob is reported as a storage error (so the
/// receive loop does not ACK it), and — crucially — a retry succeeds instead of
/// wedging on a left-open transaction (plan §REVISION C4).
// @internal
#[test]
fn genesis_receive_survives_a_commit_failure_and_retry_succeeds() {
    let alice = Identity::create("Alice", 0);
    let shared_bytes = [0x88u8; 32];
    let (bob, alice_id, broadcast) = session_less_pair(&alice, shared_bytes);
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let blob = genesis_alert_blob(&alice, &bob_pk, shared_bytes, &broadcast, [0xABu8; 32]);

    // Arm a one-shot COMMIT fault: the accept transaction will fail to commit.
    bob.storage().arm_commit_fault();
    let err = process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &blob)
        .expect_err("a commit failure must surface as an error");
    assert!(
        matches!(err, CardUpdateError::Storage(_)),
        "a commit failure must surface as a (non-ACKed) storage error, got {err:?}"
    );
    assert!(
        bob.storage()
            .safety_alerts()
            .load_unsurfaced_facts()
            .unwrap()
            .is_empty(),
        "a rolled-back accept must persist no durable fact"
    );

    // The fault self-disarmed; the retry must succeed — proving the transaction
    // was rolled back, not left open to wedge the next BEGIN IMMEDIATE.
    let outcome =
        process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &blob)
            .expect("the retry after a rolled-back commit must succeed, not wedge");
    assert!(matches!(outcome, ReceiveOutcome::Alert(_)));
    assert_eq!(
        bob.storage()
            .safety_alerts()
            .load_unsurfaced_facts()
            .unwrap()
            .len(),
        1,
        "the retry must durably persist the alert exactly once"
    );
}

/// An ordinary (non-genesis) alert blob as the exchanging device A1 would send
/// it over its established `[0;32]` session: signed by the Alice identity,
/// ratchet-encrypted with A1's initiator chain.
fn a1_session_alert_blob(
    a1_ratchet: &mut DoubleRatchetState,
    alice: &Identity,
    bob_pk: &[u8; 32],
    nonce: [u8; 32],
    timestamp: u64,
) -> Vec<u8> {
    let alert = SafetyAlertPayload::new(
        AlertKind::Emergency,
        "from primary".to_string(),
        timestamp,
        None,
        nonce,
        alice,
        bob_pk,
    )
    .expect("alert construction");
    let inner = VersionedPayload::encode_alert(&alert);
    let message = a1_ratchet.encrypt(&inner).expect("ratchet encrypt");
    serde_json::to_vec(&message).unwrap()
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// Guarded invariant 1 (ADR-064 Amendment 2026-07-24): a sibling's genesis
/// alert must NOT re-seat the recipient's established `[0;32]` session — doing
/// so silently severs the exchanging device's channel, and that device is the
/// sole card mediator for the relationship
/// (`problems/2026-07-24-genesis-reseat-severs-live-primary-channel`).
/// Sequence: A1 message decrypts (control) → A2 genesis alert surfaces →
/// A1's NEXT message on the same chain must still decrypt.
// @internal
#[test]
fn established_primary_channel_survives_sibling_genesis_alert() {
    let alice = Identity::create("Alice", 0);
    let shared_bytes = [0xA1u8; 32];
    let (bob, alice_id, broadcast) = session_less_pair(&alice, shared_bytes);
    let bob_pk = *bob.identity().unwrap().signing_public_key();

    // A1 <-> Bob established session pair over the legacy [0;32] slot.
    let (mut a1_ratchet, bob_ratchet) = setup_ratchets(&SymmetricKey::from_bytes(shared_bytes));
    bob.storage()
        .ratchets()
        .save_ratchet_state_for_device(&alice_id, &[0; 32], &bob_ratchet, false)
        .unwrap();

    // Control: A1's ordinary session message reaches Bob.
    let first = a1_session_alert_blob(&mut a1_ratchet, &alice, &bob_pk, [0x01u8; 32], 7_000);
    let outcome =
        process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &first)
            .expect("A1's message over the established session must be received");
    match outcome {
        ReceiveOutcome::Alert(a) => assert_eq!(a.message, "from primary"),
        other => panic!("expected A1's alert, got {other:?}"),
    }

    // A2 (session-less sibling) raises a genesis alert; it must surface.
    let genesis = genesis_alert_blob(&alice, &bob_pk, shared_bytes, &broadcast, [0x02u8; 32]);
    let outcome =
        process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &genesis)
            .expect("the sibling's genesis alert must be received");
    match outcome {
        ReceiveOutcome::Alert(a) => assert_eq!(a.kind, AlertKind::Emergency),
        other => panic!("expected the sibling's alert, got {other:?}"),
    }

    // The still-alive A1 sends its NEXT message on the same chain. If the
    // genesis receive re-seated [0;32], this fails DecryptionFailed and the
    // primary's channel is silently dead.
    let second = a1_session_alert_blob(&mut a1_ratchet, &alice, &bob_pk, [0x03u8; 32], 7_100);
    let outcome =
        process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &second)
            .expect("A1's channel must survive the sibling's genesis alert");
    match outcome {
        ReceiveOutcome::Alert(a) => assert_eq!(a.message, "from primary"),
        other => panic!("expected A1's post-genesis alert, got {other:?}"),
    }

    // Exactly the one declined re-seat (the sibling's genesis over A1's live
    // session) is counted — the F4-urgency signal, not an approximation.
    assert_eq!(
        bob.storage().genesis_limits().reseat_skips().unwrap(),
        1,
        "the declined re-seat must increment the skip counter exactly once"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// Guarded invariant 2 (ADR-064 Amendment 2026-07-24): the alert send path
/// must NOT persist the genesis initiator session. If it does, the sibling's
/// second alert rides an ordinary ratchet message a guarded receiver cannot
/// decrypt — ACKed and silently lost: exactly one alert per sibling, ever.
/// The absence of the persisted row IS the contract here, not an
/// implementation detail.
// @internal
#[test]
fn alert_send_leaves_no_genesis_session_behind() {
    let (secondary, bob_id) = secondary_device_after_owner_sync();
    let mut secondary = secondary;

    secondary
        .configure_emergency_broadcast(vec![bob_id.clone()], "check on me".into(), false)
        .unwrap();
    let result = secondary.send_emergency_broadcast().unwrap();
    assert_eq!(result.sent, 1, "the genesis alert must be queued");

    assert!(
        secondary
            .storage()
            .ratchets()
            .load_ratchet_state_for_device(&bob_id, &[0; 32])
            .unwrap()
            .is_none(),
        "the alert send path must not persist the genesis initiator session — \
         every sibling alert must re-genesis so a guarded receiver can open it"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// Pairing pin: a SECOND alert from the same session-less sibling must reach
/// the recipient. Green today (via the unguarded re-seat + persisted sender
/// session); it is the cell a receive-only guard would silently break, so it
/// must stay green through the two-sided change (both alerts arrive as
/// self-contained genesis envelopes once the send-side skip lands).
// @internal
#[test]
fn repeat_sibling_alerts_keep_reaching_recipient() {
    use std::time::{Duration, SystemTime};
    use vauchi_core::api::emergency::BROADCAST_COOLDOWN_SECS;
    use vauchi_core::clock::{Clock, FakeClock};
    use vauchi_core::rng::{OsSecureRng, SecureRng};

    // A fake clock so the second broadcast can pass the send cooldown
    // without a real-time wait (CC-06).
    let fake_clock = std::sync::Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    let clock: std::sync::Arc<dyn Clock> = fake_clock.clone();
    let rng: std::sync::Arc<dyn SecureRng> = OsSecureRng::shared();
    let mut alice_secondary =
        Vauchi::in_memory_with_clock_and_rng(clock, rng).expect("in-memory Vauchi");
    alice_secondary.create_identity("Alice").unwrap();
    let bob = create_vauchi_with_identity("Bob");
    let alice_pk = *alice_secondary.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let shared_bytes = [0xB2u8; 32];

    let bob_contact_at_alice = Contact::from_exchange(
        bob_pk,
        ContactCard::new("Bob"),
        SymmetricKey::from_bytes(shared_bytes),
        0,
    );
    let bob_id_at_alice = bob_contact_at_alice.id().to_string();
    alice_secondary.add_contact(bob_contact_at_alice).unwrap();

    let alice_contact_at_bob = Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        SymmetricKey::from_bytes(shared_bytes),
        0,
    );
    let alice_id_at_bob = alice_contact_at_bob.id().to_string();
    bob.add_contact(alice_contact_at_bob).unwrap();

    alice_secondary
        .configure_emergency_broadcast(vec![bob_id_at_alice.clone()], "first alarm".into(), false)
        .unwrap();
    assert_eq!(alice_secondary.send_emergency_broadcast().unwrap().sent, 1);
    let first_blob = alice_secondary
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap()
        .into_iter()
        .find(|u| u.contact_id == bob_id_at_alice)
        .expect("first queued alert")
        .payload;
    let outcome = process_single_card_update(
        bob.identity().unwrap(),
        bob.storage(),
        &alice_id_at_bob,
        &first_blob,
    )
    .expect("the first sibling alert must be received");
    match outcome {
        ReceiveOutcome::Alert(a) => assert_eq!(a.message, "first alarm"),
        other => panic!("expected the first alert, got {other:?}"),
    }

    // The coerced user re-raises the alarm from the same device, past the
    // send cooldown.
    fake_clock.advance(Duration::from_secs(BROADCAST_COOLDOWN_SECS + 1));
    alice_secondary
        .configure_emergency_broadcast(vec![bob_id_at_alice.clone()], "second alarm".into(), false)
        .unwrap();
    assert_eq!(alice_secondary.send_emergency_broadcast().unwrap().sent, 1);
    let second_blob = alice_secondary
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap()
        .into_iter()
        .filter(|u| u.contact_id == bob_id_at_alice)
        .map(|u| u.payload)
        .find(|p| p != &first_blob)
        .expect("a second, distinct queued alert");
    let outcome = process_single_card_update(
        bob.identity().unwrap(),
        bob.storage(),
        &alice_id_at_bob,
        &second_blob,
    )
    .expect("the second sibling alert must be received — never one-alert-ever");
    match outcome {
        ReceiveOutcome::Alert(a) => assert_eq!(a.message, "second alarm"),
        other => panic!("expected the second alert, got {other:?}"),
    }
    assert_eq!(
        bob.storage()
            .safety_alerts()
            .load_unsurfaced_facts()
            .unwrap()
            .len(),
        2,
        "both alerts must persist as distinct durable facts"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// A signed alert reusing an existing fact's nonce with DIFFERENT bytes is a
/// deterministic integrity conflict: the accept transaction rolls back, the
/// pre-existing fact is untouched, and the receive returns the ACKable
/// `FactConflict` (retrying can never resolve it) rather than a transient
/// storage failure that would loop forever (plan §REVISION F9/C3).
// @internal
#[test]
fn genesis_receive_rejects_a_conflicting_fact_and_rolls_back() {
    let alice = Identity::create("Alice", 0);
    let shared_bytes = [0x99u8; 32];
    let (bob, alice_id, broadcast) = session_less_pair(&alice, shared_bytes);
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let nonce = [0xCDu8; 32];

    // Pre-seed a DIFFERENT fact under the same (contact, nonce) WITHOUT its
    // replay row, so the receive reaches the fact comparator (normally the
    // replay check fires first because the two are written together).
    bob.storage()
        .safety_alerts()
        .insert_fact_if_absent(&alice_id, &nonce, b"pre-existing different bytes", 100)
        .unwrap();

    let blob = genesis_alert_blob(&alice, &bob_pk, shared_bytes, &broadcast, nonce);
    let err = process_single_card_update(bob.identity().unwrap(), bob.storage(), &alice_id, &blob)
        .expect_err("a conflicting fact must be rejected");
    assert!(
        matches!(err, CardUpdateError::FactConflict),
        "a same-nonce/different-bytes conflict must be the deterministic FactConflict, got {err:?}"
    );

    // The pre-existing fact is untouched and no replay nonce was burned — the
    // whole accept transaction rolled back.
    let facts = bob
        .storage()
        .safety_alerts()
        .load_unsurfaced_facts()
        .unwrap();
    assert_eq!(facts.len(), 1, "no second fact was created");
    assert_eq!(
        facts[0].signed_payload, b"pre-existing different bytes",
        "the original fact must be untouched by the rejected accept"
    );
}
