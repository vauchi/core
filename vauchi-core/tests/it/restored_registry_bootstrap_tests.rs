// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Unknown-device registry bootstrap retains authentication and atomic retry.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::common::linked_peer::link_contacts;
use vauchi_core::Vauchi;
use vauchi_core::api::vauchi::process_received_blobs;
use vauchi_core::api::{
    CardUpdateError, ReceiveOutcome, process_single_card_update,
    process_single_card_update_for_device,
};
use vauchi_core::clock::FakeClock;
use vauchi_core::crypto::ratchet::RatchetMessage;
use vauchi_core::network::mailbox_token::{
    compute_device_mailbox_token, current_day_epoch, token_hex,
};
use vauchi_core::network::origin_hint::{SCOPE_CARD_DELTA, seal_origin_hint};
use vauchi_core::rng::DeterministicRng;
use vauchi_core::storage::GENESIS_CONTACT_ATTEMPTS_PER_WINDOW;
use vauchi_core::sync::registry_activation::ActivationState;

const BACKUP_PASSWORD: &str = "correct-horse-battery-staple-resumption";

struct Pair {
    alice: Vauchi,
    bob: Vauchi,
    bob_at_alice: String,
    alice_at_bob: String,
}

#[derive(Debug, PartialEq, Eq)]
struct Facts {
    registry: Option<u64>,
    activation: Option<(ActivationState, Option<u64>, Option<u64>)>,
    owed: (bool, u32),
    contact_name: String,
}

fn facts(receiver: &Vauchi, sender_id: &str) -> Facts {
    let owed = receiver.storage().ux().load_own_card_repropagate().unwrap();
    Facts {
        registry: receiver
            .storage()
            .device()
            .load_contact_device_registry(sender_id)
            .unwrap()
            .map(|r| r.version()),
        activation: receiver
            .storage()
            .registry_activation()
            .load_activation(sender_id)
            .unwrap()
            .map(|a| (a.state(), a.our_version_acked(), a.peer_version_held())),
        owed: (owed.needs_repropagate, owed.failed_attempts),
        contact_name: receiver
            .get_contact(sender_id)
            .unwrap()
            .unwrap()
            .display_name()
            .to_string(),
    }
}

fn pair() -> Pair {
    let clock = Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
    ));
    let mut alice = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
    alice.create_identity("Alice").unwrap();
    let mut bob = Vauchi::in_memory_with_clock(clock).unwrap();
    bob.create_identity("Bob").unwrap();
    let linked = link_contacts(&bob, &alice, "Alice");
    Pair {
        alice,
        bob,
        bob_at_alice: linked.host_contact_id,
        alice_at_bob: linked.peer_contact_id,
    }
}

fn push(p: &Pair) -> Vec<u8> {
    assert_eq!(p.alice.queue_registry_pushes().unwrap(), 1);
    p.alice
        .storage()
        .pending()
        .get_pending_updates(&p.bob_at_alice)
        .unwrap()
        .into_iter()
        .next()
        .expect("registry push queued")
        .payload
}

fn ack(p: &Pair) -> Vec<u8> {
    let payload = push(p);
    let reply = match process_single_card_update(
        p.bob.identity().unwrap(),
        p.bob.storage(),
        &p.alice_at_bob,
        &payload,
    )
    .unwrap()
    {
        ReceiveOutcome::RegistryPushReceived(reply) => reply,
        other => panic!("expected registry push, got {other:?}"),
    };
    p.bob.queue_registry_ack(&reply).unwrap();
    let update = p
        .bob
        .storage()
        .pending()
        .get_pending_updates(&p.alice_at_bob)
        .unwrap()
        .into_iter()
        .next()
        .expect("registry ACK queued");
    assert_eq!(
        update.target_device_id.as_ref(),
        Some(p.alice.identity().unwrap().device_id())
    );
    update.payload
}

fn receive_ack(p: &Pair, payload: &[u8]) -> Result<ReceiveOutcome, CardUpdateError> {
    process_single_card_update_for_device(
        p.alice.identity().unwrap(),
        p.alice.storage(),
        &p.bob_at_alice,
        p.bob.identity().unwrap().device_id(),
        payload,
    )
}

fn exhaust_budget(p: &Pair) {
    for _ in 0..GENESIS_CONTACT_ATTEMPTS_PER_WINDOW {
        assert!(
            p.alice
                .storage()
                .genesis_limits()
                .consume_decrypt_budget(&p.bob_at_alice)
                .unwrap()
        );
    }
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
#[test]
fn authenticated_origin_bootstraps_unknown_registry_without_using_alert_budget() {
    let p = pair();
    let payload = ack(&p);
    exhaust_budget(&p);
    let contact = p.alice.get_contact(&p.bob_at_alice).unwrap().unwrap();
    let key = contact.shared_key().unwrap();
    let identity = p.alice.identity().unwrap();
    let token = token_hex(&compute_device_mailbox_token(
        key.as_bytes(),
        identity.signing_public_key(),
        identity.device_id(),
        current_day_epoch(p.alice.clock().unix_seconds()),
    ));
    let hint = seal_origin_hint(
        key.as_bytes(),
        p.bob.identity().unwrap().device_id(),
        &token,
        &payload,
        SCOPE_CARD_DELTA,
        &DeterministicRng::from_seed(1247),
    )
    .unwrap();
    let outcomes = process_received_blobs(
        identity,
        p.alice.storage(),
        &[contact],
        vec![("registry-ack".into(), token, payload, Some(hint))],
    );
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].decrypted, "{:?}", outcomes[0]);
    assert_eq!(
        facts(&p.alice, &p.bob_at_alice).activation.unwrap().0,
        ActivationState::Active
    );
    assert!(
        !p.alice
            .storage()
            .genesis_limits()
            .consume_decrypt_budget(&p.bob_at_alice)
            .unwrap()
    );
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
#[test]
fn unauthenticated_unknown_device_route_respects_exhausted_budget() {
    let p = pair();
    let payload = ack(&p);
    exhaust_budget(&p);
    let before = facts(&p.alice, &p.bob_at_alice);
    let error = receive_ack(&p, &payload).unwrap_err();
    assert!(
        matches!(error, CardUpdateError::GenesisRateLimited),
        "{error:?}"
    );
    assert_eq!(facts(&p.alice, &p.bob_at_alice), before);
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
#[test]
fn tampered_unknown_device_genesis_changes_no_registry_or_activation() {
    let p = pair();
    let payload = ack(&p);
    let mut message: RatchetMessage = serde_json::from_slice(&payload).unwrap();
    *message.ciphertext.last_mut().unwrap() ^= 0x80;
    let before = facts(&p.alice, &p.bob_at_alice);
    let error = receive_ack(&p, &serde_json::to_vec(&message).unwrap()).unwrap_err();
    assert!(
        matches!(error, CardUpdateError::NoRatchetState),
        "{error:?}"
    );
    assert_eq!(facts(&p.alice, &p.bob_at_alice), before);
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
#[test]
fn held_registry_missing_the_sender_accepts_its_authenticated_new_registry() {
    let mut p = pair();
    let identity = p.bob.identity().unwrap();
    p.alice
        .storage()
        .device()
        .save_contact_device_registry(
            &p.bob_at_alice,
            &p.bob.own_registry_broadcast(identity).unwrap(),
            identity.signing_public_key(),
            p.alice.clock().unix_seconds(),
        )
        .unwrap();
    let backup = p.bob.export_full_backup(BACKUP_PASSWORD).unwrap();
    let mut replacement = Vauchi::in_memory_with_clock(p.bob.clock().clone()).unwrap();
    replacement
        .import_full_backup(&backup, BACKUP_PASSWORD)
        .unwrap();
    p.bob = replacement;
    let payload = ack(&p);
    let outcome = receive_ack(&p, &payload).unwrap();
    assert!(
        matches!(outcome, ReceiveOutcome::RegistryAckReceived { .. }),
        "{outcome:?}"
    );
    let registry = p
        .alice
        .storage()
        .device()
        .load_contact_device_registry(&p.bob_at_alice)
        .unwrap()
        .unwrap();
    assert!(registry.contains_device(p.bob.identity().unwrap().device_id()));
    assert_eq!(
        facts(&p.alice, &p.bob_at_alice).activation.unwrap().0,
        ActivationState::Active
    );
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
#[test]
fn unknown_device_ack_commit_failure_rolls_back_and_retry_succeeds() {
    let p = pair();
    let payload = ack(&p);
    let before = facts(&p.alice, &p.bob_at_alice);
    p.alice.storage().arm_commit_fault();
    let error = receive_ack(&p, &payload).unwrap_err();
    assert!(matches!(error, CardUpdateError::Storage(_)), "{error:?}");
    assert_eq!(facts(&p.alice, &p.bob_at_alice), before);
    let outcome = receive_ack(&p, &payload).expect("retry on the same connection must succeed");
    assert!(
        matches!(outcome, ReceiveOutcome::RegistryAckReceived { .. }),
        "{outcome:?}"
    );
    assert_eq!(
        facts(&p.alice, &p.bob_at_alice).activation.unwrap().0,
        ActivationState::Active
    );
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
#[test]
fn genesis_push_commit_failure_rolls_back_and_retry_succeeds() {
    let p = pair();
    let payload = push(&p);
    let before = facts(&p.bob, &p.alice_at_bob);
    p.bob.storage().arm_commit_fault();
    let receive = || {
        process_single_card_update(
            p.bob.identity().unwrap(),
            p.bob.storage(),
            &p.alice_at_bob,
            &payload,
        )
    };
    let error = receive().unwrap_err();
    assert!(matches!(error, CardUpdateError::Storage(_)), "{error:?}");
    assert_eq!(facts(&p.bob, &p.alice_at_bob), before);
    let outcome = receive().expect("retry on the same connection must succeed");
    assert!(
        matches!(outcome, ReceiveOutcome::RegistryPushReceived(_)),
        "{outcome:?}"
    );
    assert_eq!(
        facts(&p.bob, &p.alice_at_bob).registry,
        Some(
            p.alice
                .own_registry_broadcast(p.alice.identity().unwrap())
                .unwrap()
                .version()
        )
    );
}
