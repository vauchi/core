// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A Link-mode contact must receive later card updates (ADR-050: the
//! exchange is symmetric and updatable). The Double Ratchet responder
//! cannot send before the initiator's first message, so completing a Link
//! exchange has to arm the own-card repropagation marker on both sides:
//! the initiator's next sync then primes the responder's sending chain,
//! exactly as the QR/BLE path does in `finish_exchange`.

use vauchi_core::Vauchi;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::exchange::key_order::is_initiator;
use vauchi_core::exchange::link_mode::serialize_card_payload_v2;

fn vauchi_with_identity(name: &str) -> Vauchi {
    let mut v = Vauchi::in_memory().expect("in-memory vauchi");
    v.create_identity(name).expect("create identity");
    v
}

fn link_payload(v: &Vauchi) -> (Vec<u8>, X3DHKeyPair) {
    let identity = v.identity().expect("identity exists");
    let x3dh = X3DHKeyPair::generate();
    let card = ContactCard::new(identity.display_name());
    let payload = serialize_card_payload_v2(
        identity.signing_public_key(),
        identity.signing_keypair(),
        x3dh.public_key(),
        "https://relay.example",
        &card,
    );
    (payload, x3dh)
}

fn signing_key(v: &Vauchi) -> [u8; 32] {
    *v.identity().expect("identity exists").signing_public_key()
}

/// Completes a Link exchange both ways and returns
/// `(initiator, responder, responder_id_on_initiator, initiator_id_on_responder)`.
fn linked_pair() -> (Vauchi, Vauchi, String, String) {
    let a = vauchi_with_identity("Alice");
    let b = vauchi_with_identity("Bob");
    let (a_payload, a_x3dh) = link_payload(&a);
    let (b_payload, b_x3dh) = link_payload(&b);
    let b_on_a = a
        .complete_link_exchange(&b_payload, &a_x3dh)
        .expect("alice completes bob's bootstrap");
    let a_on_b = b
        .complete_link_exchange(&a_payload, &b_x3dh)
        .expect("bob completes alice's bootstrap");
    if is_initiator(&signing_key(&a), &signing_key(&b)) {
        (a, b, b_on_a, a_on_b)
    } else {
        (b, a, a_on_b, b_on_a)
    }
}

fn pending_count(v: &Vauchi, contact_id: &str) -> usize {
    v.storage()
        .pending()
        .get_pending_updates(contact_id)
        .expect("pending queue readable")
        .len()
}

fn own_field_id(v: &Vauchi, label: &str) -> String {
    v.own_card()
        .expect("own card ok")
        .expect("own card exists")
        .fields()
        .iter()
        .find(|f| f.label() == label)
        .map(|f| f.id().to_string())
        .expect("field exists")
}

// @scenario: link_exchange :: Completing a Link exchange primes the update channel
#[test]
fn completing_a_link_exchange_arms_repropagation_so_the_initiator_primes_the_peer() {
    let (initiator, responder, responder_id, initiator_id) = linked_pair();

    for side in [&initiator, &responder] {
        let state = side
            .storage()
            .ux()
            .load_own_card_repropagate()
            .expect("marker readable");
        assert!(
            state.needs_repropagate,
            "a Link exchange must arm the own-card repropagation marker"
        );
    }

    initiator
        .run_owed_repropagation()
        .expect("initiator repropagation runs");
    assert_eq!(
        pending_count(&initiator, &responder_id),
        1,
        "the initiator's first sync must queue its card for the new Link contact"
    );

    responder
        .run_owed_repropagation()
        .expect("responder repropagation runs");
    assert_eq!(
        pending_count(&responder, &initiator_id),
        0,
        "the responder cannot send before the initiator's first message"
    );
    let state = responder
        .storage()
        .ux()
        .load_own_card_repropagate()
        .expect("marker readable");
    assert!(
        state.should_run(),
        "the deferred responder send must stay armed for the next sync"
    );
}

// @scenario: link_exchange :: A visible own-card edit reaches a Link contact
#[test]
fn visible_field_added_after_link_exchange_queues_a_delta_for_the_peer() {
    let (initiator, _responder, responder_id, _) = linked_pair();

    initiator
        .add_own_field(ContactField::new(
            FieldType::Email,
            "Work",
            "alice@example.com",
            0,
        ))
        .expect("add field");
    let field_id = own_field_id(&initiator, "Work");
    initiator
        .set_field_shown(&field_id, true)
        .expect("make the field visible to all contacts");

    initiator
        .run_owed_repropagation()
        .expect("repropagation runs");

    assert!(
        pending_count(&initiator, &responder_id) > 0,
        "a field visible to everyone must queue a card delta for the Link contact"
    );
}

fn deliver_all(from: &Vauchi, to: &Vauchi, to_id_at_from: &str, from_id_at_to: &str) -> usize {
    let pending = from
        .storage()
        .pending()
        .get_pending_updates(to_id_at_from)
        .expect("pending queue readable");
    for update in &pending {
        vauchi_core::api::process_single_card_update(
            to.identity().expect("identity"),
            to.storage(),
            from_id_at_to,
            &update.payload,
        )
        .unwrap_or_else(|e| panic!("delivery failed: {e:?}"));
        from.storage()
            .pending()
            .delete_pending_update(&update.id)
            .expect("dequeue delivered update");
    }
    pending.len()
}

fn peer_card_value(v: &Vauchi, contact_id: &str, label: &str) -> Option<String> {
    v.get_contact(contact_id)
        .expect("get ok")
        .expect("contact exists")
        .card()
        .fields()
        .iter()
        .find(|f| f.label() == label)
        .map(|f| f.value().to_string())
}

// @scenario: link_exchange :: Edits from both sides of a Link exchange converge
#[test]
fn edits_from_both_sides_of_a_link_exchange_reach_the_peer() {
    let (initiator, responder, responder_id, initiator_id) = linked_pair();

    // Initiator's first sync primes the responder's sending chain.
    initiator.run_owed_repropagation().expect("initiator pass");
    assert_eq!(
        deliver_all(&initiator, &responder, &responder_id, &initiator_id),
        1
    );
    // The responder's deferred pass now goes through.
    responder.run_owed_repropagation().expect("responder pass");
    assert_eq!(
        deliver_all(&responder, &initiator, &initiator_id, &responder_id),
        1,
        "once primed, the responder's owed pass must queue its card"
    );

    // Responder edits a field visible to everyone.
    responder
        .add_own_field(ContactField::new(
            FieldType::Email,
            "Work",
            "bob@example.com",
            0,
        ))
        .expect("add field");
    let field_id = own_field_id(&responder, "Work");
    responder
        .set_field_shown(&field_id, true)
        .expect("visible to all");
    responder.run_owed_repropagation().expect("responder pass");
    assert!(deliver_all(&responder, &initiator, &initiator_id, &responder_id) >= 1);
    assert_eq!(
        peer_card_value(&initiator, &responder_id, "Work").as_deref(),
        Some("bob@example.com")
    );

    // Initiator edits too; the responder must see it.
    initiator
        .add_own_field(ContactField::new(
            FieldType::Email,
            "Work",
            "alice@example.com",
            0,
        ))
        .expect("add field");
    let field_id = own_field_id(&initiator, "Work");
    initiator
        .set_field_shown(&field_id, true)
        .expect("visible to all");
    initiator.run_owed_repropagation().expect("initiator pass");
    assert!(deliver_all(&initiator, &responder, &responder_id, &initiator_id) >= 1);
    assert_eq!(
        peer_card_value(&responder, &initiator_id, "Work").as_deref(),
        Some("alice@example.com")
    );
}
