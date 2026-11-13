// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Full restore must resume both directions through the new device's queues.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::common::meeting::meet;
use vauchi_core::api::{ReceiveOutcome, process_single_card_update_for_device};
use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::{ContactField, FieldType, Vauchi};

const PASSWORD: &str = "correct-horse-battery-staple-resumption";

fn deliver(from: &Vauchi, to: &Vauchi, to_id: &str, from_id: &str) {
    for update in from.storage().pending().get_pending_updates(to_id).unwrap() {
        from.storage()
            .pending()
            .delete_pending_update(&update.id)
            .unwrap();
        if update
            .target_device_id
            .is_some_and(|target| &target != to.identity().unwrap().device_id())
        {
            continue;
        }
        let origin = update
            .target_device_id
            .map(|_| *from.identity().unwrap().device_id())
            .unwrap_or([0; 32]);
        let outcome = process_single_card_update_for_device(
            to.identity().unwrap(),
            to.storage(),
            from_id,
            &origin,
            &update.payload,
        )
        .unwrap_or_else(|error| {
            panic!(
                "{} (device index {}) to {} (device index {}): {error:?}",
                from.identity().unwrap().display_name(),
                from.identity().unwrap().device_index(),
                to.identity().unwrap().display_name(),
                to.identity().unwrap().device_index(),
            )
        });
        match outcome {
            ReceiveOutcome::RegistryPushReceived(reply)
            | ReceiveOutcome::RegistryAckReceived {
                reply: Some(reply), ..
            } => {
                to.queue_registry_ack(&reply).unwrap();
            }
            ReceiveOutcome::CardDelta | ReceiveOutcome::RegistryAckReceived { reply: None, .. } => {
            }
            other => panic!("unexpected queued payload: {other:?}"),
        }
    }
}

fn sync_pair(alice: &Vauchi, bob: &Vauchi, bob_at_alice: &str, alice_at_bob: &str) {
    alice.queue_registry_pushes().unwrap();
    alice.run_owed_repropagation().unwrap();
    deliver(alice, bob, bob_at_alice, alice_at_bob);
    bob.queue_registry_pushes().unwrap();
    bob.run_owed_repropagation().unwrap();
    deliver(bob, alice, alice_at_bob, bob_at_alice);
}

fn add_public_field(owner: &Vauchi, label: &str, value: &str) {
    let field = ContactField::new(FieldType::Email, label, value, owner.clock().unix_seconds());
    let id = field.id().to_string();
    owner.add_own_field(field).unwrap();
    owner.set_own_field_public(&id).unwrap();
}

fn edit(owner: &Vauchi, label: &str, value: &str) {
    let mut card = owner.own_card().unwrap().unwrap();
    let id = card
        .fields()
        .iter()
        .find(|field| field.label() == label)
        .unwrap()
        .id()
        .to_string();
    card.update_field_value(&id, value, owner.clock().unix_seconds())
        .unwrap();
    owner.update_own_card(&card).unwrap();
}

fn received(owner: &Vauchi, contact_id: &str, label: &str) -> Option<String> {
    owner
        .get_contact(contact_id)
        .unwrap()
        .unwrap()
        .card()
        .fields()
        .iter()
        .find(|field| field.label() == label)
        .map(|field| field.value().to_string())
}

fn run_restore(alice_initiates: bool) -> (Option<String>, Option<String>) {
    let clock = Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
    ));
    let mut bob = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
    bob.create_identity("Bob").unwrap();
    let alice = loop {
        let mut candidate = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
        candidate.create_identity("Alice").unwrap();
        if (candidate.identity().unwrap().signing_public_key()
            < bob.identity().unwrap().signing_public_key())
            == alice_initiates
        {
            break candidate;
        }
    };
    let (alice_at_bob, bob_at_alice) = meet(&bob, &alice, false);
    for (receiver, peer, contact_id) in
        [(&alice, &bob, &bob_at_alice), (&bob, &alice, &alice_at_bob)]
    {
        let identity = peer.identity().unwrap();
        receiver
            .storage()
            .device()
            .save_contact_device_registry(
                contact_id,
                &peer.own_registry_broadcast(identity).unwrap(),
                identity.signing_public_key(),
                clock.unix_seconds(),
            )
            .unwrap();
    }
    add_public_field(&alice, "AliceEmail", "old-alice@example.test");
    add_public_field(&bob, "BobEmail", "old-bob@example.test");
    for _ in 0..3 {
        sync_pair(&alice, &bob, &bob_at_alice, &alice_at_bob);
    }
    assert_eq!(
        received(&bob, &alice_at_bob, "AliceEmail").as_deref(),
        Some("old-alice@example.test")
    );
    assert_eq!(
        received(&alice, &bob_at_alice, "BobEmail").as_deref(),
        Some("old-bob@example.test")
    );
    let backup = alice.export_full_backup(PASSWORD).unwrap();
    drop(alice);
    clock.advance(Duration::from_secs(10));
    let mut restored = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
    restored.import_full_backup(&backup, PASSWORD).unwrap();
    edit(&restored, "AliceEmail", "new-alice@example.test");
    edit(&bob, "BobEmail", "new-bob@example.test");
    for _ in 0..6 {
        sync_pair(&restored, &bob, &bob_at_alice, &alice_at_bob);
    }
    (
        received(&bob, &alice_at_bob, "AliceEmail"),
        received(&restored, &bob_at_alice, "BobEmail"),
    )
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
#[test]
fn restored_install_resumes_both_card_directions_through_its_own_queues() {
    assert_eq!(
        run_restore(true),
        (
            Some("new-alice@example.test".into()),
            Some("new-bob@example.test".into())
        ),
    );
}

// @scenario: backup_format_versioning :: Restored contacts continue exchanging updates
#[test]
fn restored_responder_resumes_both_card_directions_through_its_own_queues() {
    assert_eq!(
        run_restore(false),
        (
            Some("new-alice@example.test".into()),
            Some("new-bob@example.test".into())
        ),
    );
}
