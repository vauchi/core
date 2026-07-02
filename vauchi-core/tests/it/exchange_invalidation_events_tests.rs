// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! In-person exchange persist dispatches screen-invalidation events.
//!
//! `Vauchi::update_contact` is the persist seam every exchange transport
//! converges on (BLE + multi-stage via `save_exchanged_contact`, QR
//! directly). Without a `VauchiEvent` dispatched there, the
//! `affected_screens` → `on_screens_invalidated` bridge never fires and
//! frontends render a stale contacts list until process death — the
//! third sibling of the sync-UI invalidation family (relay receive
//! 9468ad3d, device-sync arms 5d13a463). Problem record:
//! 2026-07-01-android-contacts-list-stale-after-mutation.
//! Feature: contact_exchange.feature

use crate::common;

use std::sync::{Arc, Mutex};

use common::helpers::{create_vauchi_with_card, setup_ratchets};
use vauchi_core::api::VauchiEvent;
use vauchi_core::types::EventOrigin;
use vauchi_core::{Contact, FieldType, SymmetricKey, Vauchi};

fn capture_events(vauchi: &Vauchi) -> Arc<Mutex<Vec<VauchiEvent>>> {
    let events: Arc<Mutex<Vec<VauchiEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    vauchi.add_event_handler(Arc::new(move |event: VauchiEvent| {
        sink.lock().unwrap().push(event);
    }));
    events
}

fn contact_events(events: &[VauchiEvent]) -> Vec<&VauchiEvent> {
    events
        .iter()
        .filter(|e| {
            matches!(
                e,
                VauchiEvent::ContactAdded { .. } | VauchiEvent::ContactUpdated { .. }
            )
        })
        .collect()
}

/// A first exchange persists a NEW contact → exactly one
/// `ContactAdded { origin: Local }` so `affected_screens` invalidates the
/// contacts list. RED while `Vauchi::update_contact` is a bare save.
// @scenario: contact_exchange :: First exchange persists the contact
#[test]
fn first_exchange_dispatches_contact_added_local() {
    let alice = create_vauchi_with_card("Alice", vec![(FieldType::Email, "work", "a@work.com")]);
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "bob@v1.com")]);

    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let bob_card = bob.own_card().unwrap().unwrap();

    let secret = SymmetricKey::generate();
    let (a_rat, _b_rat) = setup_ratchets(&secret);
    let bob_at_alice = Contact::from_exchange(bob_pk, bob_card, secret, 0);
    let bob_id = bob_at_alice.id().to_string();

    let events = capture_events(&alice);
    alice
        .save_exchanged_contact(&bob_at_alice, &a_rat, true)
        .unwrap();

    let captured = events.lock().unwrap();
    let contact_evts = contact_events(&captured);
    assert_eq!(
        contact_evts.len(),
        1,
        "first exchange must dispatch exactly one contact event, got: {contact_evts:?}"
    );
    match contact_evts[0] {
        VauchiEvent::ContactAdded {
            contact_id, origin, ..
        } => {
            assert_eq!(
                contact_id, &bob_id,
                "event must carry the exchanged contact's id"
            );
            assert_eq!(
                origin,
                &EventOrigin::Local,
                "in-person exchange is Local origin (Synced would trigger an OS notification)"
            );
        }
        other => panic!("expected ContactAdded, got {other:?}"),
    }
}

/// A repeat exchange carrying a CHANGED card → exactly one
/// `ContactUpdated` with the changed field labels. RED while
/// `Vauchi::update_contact` is a bare save.
// @scenario: contact_exchange :: Repeat in-person exchange of the same pair
#[test]
fn repeat_exchange_with_changed_card_dispatches_contact_updated() {
    let alice = create_vauchi_with_card("Alice", vec![(FieldType::Email, "work", "a@work.com")]);
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "bob@v1.com")]);

    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let bob_card_v1 = bob.own_card().unwrap().unwrap();

    let secret1 = SymmetricKey::generate();
    let (a_rat1, _) = setup_ratchets(&secret1);
    let bob_at_alice_v1 = Contact::from_exchange(bob_pk, bob_card_v1.clone(), secret1, 0);
    let bob_id = bob_at_alice_v1.id().to_string();
    alice
        .save_exchanged_contact(&bob_at_alice_v1, &a_rat1, true)
        .unwrap();

    let bob_email_id = bob_card_v1
        .fields()
        .iter()
        .find(|f| f.label() == "personal")
        .unwrap()
        .id()
        .to_string();
    let mut bob_card_v2 = bob_card_v1.clone();
    bob_card_v2
        .update_field_value(&bob_email_id, "bob@v2.com", 1)
        .unwrap();

    let secret2 = SymmetricKey::generate();
    let (a_rat2, _) = setup_ratchets(&secret2);
    let bob_at_alice_v2 = Contact::from_exchange(bob_pk, bob_card_v2, secret2, 2);

    let events = capture_events(&alice);
    alice
        .save_exchanged_contact(&bob_at_alice_v2, &a_rat2, true)
        .unwrap();

    let captured = events.lock().unwrap();
    let contact_evts = contact_events(&captured);
    assert_eq!(
        contact_evts.len(),
        1,
        "repeat exchange with a changed card must dispatch exactly one contact event, got: {contact_evts:?}"
    );
    match contact_evts[0] {
        VauchiEvent::ContactUpdated {
            contact_id,
            changed_fields,
            ..
        } => {
            assert_eq!(
                contact_id, &bob_id,
                "event must carry the exchanged contact's id"
            );
            assert_eq!(
                changed_fields,
                &vec!["personal".to_string()],
                "changed_fields must name exactly the changed field label"
            );
        }
        other => panic!("expected ContactUpdated, got {other:?}"),
    }
}

/// A repeat exchange with an UNCHANGED card (pure rekey) → no contact
/// event: nothing on the contacts list changed, so no invalidation.
// @scenario: contact_exchange :: Repeat in-person exchange of the same pair
#[test]
fn repeat_exchange_with_unchanged_card_dispatches_no_contact_events() {
    let alice = create_vauchi_with_card("Alice", vec![(FieldType::Email, "work", "a@work.com")]);
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "bob@v1.com")]);

    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let bob_card = bob.own_card().unwrap().unwrap();

    let secret1 = SymmetricKey::generate();
    let (a_rat1, _) = setup_ratchets(&secret1);
    alice
        .save_exchanged_contact(
            &Contact::from_exchange(bob_pk, bob_card.clone(), secret1, 0),
            &a_rat1,
            true,
        )
        .unwrap();

    let secret2 = SymmetricKey::generate();
    let (a_rat2, _) = setup_ratchets(&secret2);
    let bob_at_alice_rekey = Contact::from_exchange(bob_pk, bob_card, secret2, 2);

    let events = capture_events(&alice);
    alice
        .save_exchanged_contact(&bob_at_alice_rekey, &a_rat2, true)
        .unwrap();

    let captured = events.lock().unwrap();
    let contact_evts = contact_events(&captured);
    assert_eq!(
        contact_evts.len(),
        0,
        "a pure rekey (unchanged card) must not invalidate the contacts list, got: {contact_evts:?}"
    );
}
