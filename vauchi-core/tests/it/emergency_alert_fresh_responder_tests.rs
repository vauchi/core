// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! An emergency alert reaches a contact this device has not yet heard from
//! (private#633). Right after an exchange the responder has no sending chain
//! until the initiator's first message, and nothing re-queues a deferred
//! alert, so it must not wait for one.

use crate::common;

use common::helpers::create_vauchi_with_identity;
use common::linked_peer::link_contacts;
use vauchi_core::api::vauchi::process_received_blobs;
use vauchi_core::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};

// @scenario: emergency_broadcast :: An alert reaches a contact not yet heard from
// @internal
#[test]
fn an_alert_to_a_contact_not_yet_heard_from_is_sent_and_received() {
    // Alice is the ratchet responder: Bob has sent her nothing yet.
    let mut alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let link = link_contacts(&alice, &bob, "Bob");
    alice
        .configure_emergency_broadcast(vec![link.peer_contact_id.clone()], "Help".into(), false)
        .unwrap();

    let result = alice.send_emergency_broadcast().unwrap();

    assert_eq!(
        (result.sent, result.total),
        (1, 1),
        "the alert is queued, not skipped"
    );
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let token = token_hex(&compute_mailbox_token(
        link.shared_secret.as_bytes(),
        &bob_pk,
        current_day_epoch(bob.storage().clock().unix_seconds()),
    ));
    let blobs = alice
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap()
        .into_iter()
        .map(|update| (update.id, token.clone(), update.payload, None))
        .collect();
    let contacts = bob.storage().contacts().list_contacts().unwrap();
    let outcomes = process_received_blobs(bob.identity().unwrap(), bob.storage(), &contacts, blobs);
    assert_eq!(outcomes.len(), 1);
    assert!(
        outcomes[0].alert.is_some(),
        "Bob receives the alert: {:?}",
        outcomes[0].reject_reason
    );
}
