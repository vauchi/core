// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! When the sync cycle re-pushes our registry to a contact that has not
//! acked it (private#662): once per registry version, and again only after
//! a message from that contact shows it is online. Before, a sent push was
//! re-queued on every sync, forever for a contact who never came back.

use crate::common;

use common::helpers::create_vauchi_with_identity;
use common::linked_peer::{LinkedPeer, link_contacts};
use vauchi_core::Vauchi;
use vauchi_core::api::vauchi::{BlobOutcome, process_received_blobs};
use vauchi_core::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};
use vauchi_core::sync::registry_activation::ActivationTracker;

/// Sends everything `sender` has queued: the relay accepts it, so the send
/// phase drops it from the queue.
fn send_queued(sender: &Vauchi) -> Vec<(String, Vec<u8>)> {
    let pending = sender.storage().pending();
    pending
        .get_all_pending_updates()
        .unwrap()
        .into_iter()
        .map(|update| {
            pending.mark_update_sent(&update.id).unwrap();
            (update.id, update.payload)
        })
        .collect()
}

fn deliver(
    sent: Vec<(String, Vec<u8>)>,
    recipient: &Vauchi,
    link: &LinkedPeer,
) -> Vec<BlobOutcome> {
    let recipient_pk = *recipient.identity().unwrap().signing_public_key();
    let token = token_hex(&compute_mailbox_token(
        link.shared_secret.as_bytes(),
        &recipient_pk,
        current_day_epoch(recipient.storage().clock().unix_seconds()),
    ));
    let contacts = recipient.storage().contacts().list_contacts().unwrap();
    let blobs = sent
        .into_iter()
        .map(|(id, payload)| (id, token.clone(), payload, None))
        .collect();
    process_received_blobs(
        recipient.identity().unwrap(),
        recipient.storage(),
        &contacts,
        blobs,
    )
}

// The rig loop of 2026-10-12: one blob per sync to a contact who had not
// acked yet, with nothing new to say.
// @scenario: multi_device_sync :: A sent push waits for the contact before it is sent again
// @internal
#[test]
fn a_sent_push_is_not_sent_again_until_the_contact_is_heard_from() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let link = link_contacts(&alice, &bob, "Bob");
    assert_eq!(alice.queue_registry_pushes().unwrap(), 1);
    send_queued(&alice);

    assert_eq!(
        alice.queue_registry_pushes().unwrap(),
        0,
        "nothing changed and Bob has not been heard from"
    );

    assert_eq!(bob.queue_registry_pushes().unwrap(), 1);
    let at_alice = deliver(send_queued(&bob), &alice, &link);
    assert!(at_alice[0].decrypted, "Alice hears from Bob");

    assert_eq!(
        alice.queue_registry_pushes().unwrap(),
        1,
        "a message from Bob shows he is online: push once more"
    );
    send_queued(&alice);
    assert_eq!(alice.queue_registry_pushes().unwrap(), 0);
}

// @scenario: multi_device_sync :: A registry change is pushed without waiting for the contact
// @internal
#[test]
fn a_registry_change_is_pushed_without_hearing_from_the_contact() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let link = link_contacts(&alice, &bob, "Bob");
    // Our push of an older registry version went out and was never acked.
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([1; 32], 0);
    alice
        .storage()
        .registry_activation()
        .save_activation(&link.peer_contact_id, &tracker)
        .unwrap();

    assert_eq!(alice.queue_registry_pushes().unwrap(), 1);
}

// A tracker saved before the rule (or by a sibling) may re-push once, then
// waits like any other.
// @internal
#[test]
fn a_stored_unacked_push_of_the_current_version_is_sent_once_more_then_waits() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let link = link_contacts(&alice, &bob, "Bob");
    assert_eq!(alice.queue_registry_pushes().unwrap(), 1);
    send_queued(&alice);
    let pushed = alice
        .storage()
        .registry_activation()
        .load_activation(&link.peer_contact_id)
        .unwrap()
        .unwrap()
        .outstanding_push()
        .unwrap();
    let before_the_rule = ActivationTracker::from_parts(Some(pushed), None, None);
    alice
        .storage()
        .registry_activation()
        .save_activation(&link.peer_contact_id, &before_the_rule)
        .unwrap();

    assert_eq!(alice.queue_registry_pushes().unwrap(), 1);
    send_queued(&alice);
    assert_eq!(alice.queue_registry_pushes().unwrap(), 0);
}
