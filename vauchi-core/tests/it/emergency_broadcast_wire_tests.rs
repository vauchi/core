// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! An emergency alert on the wire: shaped and padded like a card update,
//! readable only by its recipient, and not repeatable within a minute.

use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::linked_peer::link_contacts;
use vauchi_core::Vauchi;
use vauchi_core::api::{ReceiveOutcome, process_single_card_update};
use vauchi_core::clock::FakeClock;
use vauchi_core::contact_card::{ContactField, FieldType};

const ALERT: &str = "I may be in danger. Please check on me.";

/// (update type, envelope field names, ciphertext length) of each update
/// queued for `contact_id`, removing them from the queue.
fn take_queued(owner: &Vauchi, contact_id: &str) -> Vec<(String, Vec<String>, usize, Vec<u8>)> {
    let queued = owner
        .storage()
        .pending()
        .get_pending_updates(contact_id)
        .unwrap();
    queued
        .into_iter()
        .map(|update| {
            owner
                .storage()
                .pending()
                .delete_pending_update(&update.id)
                .unwrap();
            let envelope: serde_json::Value = serde_json::from_slice(&update.payload).unwrap();
            let fields = envelope.as_object().unwrap().keys().cloned().collect();
            let ciphertext = envelope["ciphertext"].as_array().unwrap().len();
            (update.update_type, fields, ciphertext, update.payload)
        })
        .collect()
}

// @scenario: emergency_broadcast :: Alert looks like normal sync traffic
// @scenario: emergency_broadcast :: Alert is encrypted end-to-end
#[test]
fn an_alert_travels_as_a_padded_card_update_only_its_recipient_can_read() {
    let mut alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let carol = create_vauchi_with_identity("Carol");
    let to_bob = link_contacts(&bob, &alice, "Alice");
    let to_carol = link_contacts(&carol, &alice, "Alice");
    alice
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "Mobile",
            "+41 79 123 45 67",
            0,
        ))
        .unwrap();
    let mobile = alice.own_card().unwrap().unwrap().fields()[0]
        .id()
        .to_string();
    alice.set_own_field_public(&mobile).unwrap();
    alice.run_owed_repropagation().unwrap();
    let card_update = take_queued(&alice, &to_bob.host_contact_id).remove(0);
    take_queued(&alice, &to_carol.host_contact_id);
    alice
        .configure_emergency_broadcast(vec![to_bob.host_contact_id.clone()], ALERT.into(), false)
        .unwrap();

    alice.send_emergency_broadcast().unwrap();

    let alerts = take_queued(&alice, &to_bob.host_contact_id);
    assert_eq!(alerts.len(), 1);
    let (kind, fields, ciphertext, payload) = &alerts[0];
    assert_eq!((kind, fields), (&card_update.0, &card_update.1));
    assert_eq!(
        ciphertext % 256,
        card_update.2 % 256,
        "both are a padding bucket plus the same overhead"
    );
    assert!(!payload.windows(ALERT.len()).any(|w| w == ALERT.as_bytes()));
    assert!(take_queued(&alice, &to_carol.host_contact_id).is_empty());
    let at_carol = process_single_card_update(
        carol.identity().unwrap(),
        carol.storage(),
        &to_carol.peer_contact_id,
        payload,
    );
    assert!(at_carol.is_err(), "Carol cannot open Bob's alert");
    let at_bob = process_single_card_update(
        bob.identity().unwrap(),
        bob.storage(),
        &to_bob.peer_contact_id,
        payload,
    )
    .unwrap();
    let ReceiveOutcome::Alert(alert) = at_bob else {
        panic!("Bob receives an alert, got {at_bob:?}");
    };
    assert_eq!(alert.message, ALERT);
}

// @scenario: emergency_broadcast :: Rate limiting emergency broadcasts
#[test]
fn a_second_alert_within_a_minute_is_refused_and_allowed_after() {
    let clock = Arc::new(FakeClock::new(
        UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    let mut alice = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
    alice.create_identity("Alice").unwrap();
    let bob = create_vauchi_with_identity("Bob");
    let to_bob = link_contacts(&bob, &alice, "Alice");
    alice
        .configure_emergency_broadcast(vec![to_bob.host_contact_id], ALERT.into(), false)
        .unwrap();
    alice.send_emergency_broadcast().unwrap();

    clock.advance(Duration::from_secs(59));
    let refused = alice.send_emergency_broadcast();
    clock.advance(Duration::from_secs(1));
    let allowed = alice.send_emergency_broadcast();

    let Err(refusal) = refused else {
        panic!("a repeat within a minute is refused, got {refused:?}");
    };
    assert!(refusal.to_string().contains("cooldown"), "{refusal}");
    assert_eq!(allowed.unwrap().sent, 1);
}
