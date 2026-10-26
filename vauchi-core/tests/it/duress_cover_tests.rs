// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A card edit made under the duress PIN queues cover: for every contact
//! and device the real edit would reach, a message of the same length,
//! which the contact opens as nothing (ADR-032, vauchi/private#811). From
//! the coercer's side the queue is the one a real edit leaves.

use proptest::prelude::*;
use vauchi_core::api::{AuthMode, ReceiveOutcome, process_single_card_update};
use vauchi_core::crypto::{RatchetMessage, SymmetricKey};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::{Contact, ContactCard, ContactField, FieldType, Vauchi};

use crate::common::helpers::create_vauchi_with_identity;

const APP_PASSWORD: &str = "my-pin-1234";
const DURESS_PIN: &str = "654321";

/// Alice, unlocked with `credential`, with Bob as a ratcheted contact —
/// and Bob holding Alice on the other end of that ratchet.
struct World {
    alice: Vauchi,
    bob: Vauchi,
    alice_id: String,
    bob_id: String,
}

fn world(credential: &str, expected_mode: AuthMode) -> World {
    let mut alice = create_vauchi_with_identity("Alice");
    alice.setup_app_password(APP_PASSWORD).unwrap();
    alice.setup_duress_password(DURESS_PIN).unwrap();
    let bob = create_vauchi_with_identity("Bob");
    let shared = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();

    let bob_contact = Contact::from_exchange(
        *bob.identity().unwrap().signing_public_key(),
        ContactCard::new("Bob"),
        shared.clone(),
        0,
    );
    let bob_id = bob_contact.id().to_string();
    alice.add_contact(bob_contact).unwrap();
    alice
        .create_ratchet_as_initiator(&bob_id, &shared, *bob_dh.public_key())
        .unwrap();

    let alice_contact = Contact::from_exchange(
        *alice.identity().unwrap().signing_public_key(),
        ContactCard::new("Alice"),
        shared.clone(),
        0,
    );
    let alice_id = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();
    bob.create_ratchet_as_responder(&alice_id, &shared, bob_dh)
        .unwrap();

    assert_eq!(alice.authenticate(credential).unwrap(), expected_mode);
    World {
        alice,
        bob,
        alice_id,
        bob_id,
    }
}

fn normal_world() -> World {
    world(APP_PASSWORD, AuthMode::Normal)
}

fn duress_world() -> World {
    world(DURESS_PIN, AuthMode::Duress)
}

#[derive(Debug, Clone)]
enum Edit {
    AddPublicField { label: String, value: String },
    AddHiddenField { label: String, value: String },
    Rename(String),
}

fn apply(alice: &mut Vauchi, edit: &Edit) {
    match edit {
        Edit::AddPublicField { label, value } => {
            let field = ContactField::new(FieldType::Email, label, value, 0);
            let field_id = field.id().to_string();
            alice.add_own_field(field).unwrap();
            alice.set_own_field_public(&field_id).unwrap();
        }
        Edit::AddHiddenField { label, value } => {
            alice
                .add_own_field(ContactField::new(FieldType::Custom, label, value, 0))
                .unwrap();
        }
        Edit::Rename(name) => alice.update_display_name(name).unwrap(),
    }
}

/// What a coercer can see of the queue: kind, padded ciphertext length and
/// device of every pending update, order-free. (The recipient is Bob in
/// both worlds; his id differs because his identity does.)
fn queue_shape(wb: &Vauchi) -> Vec<(String, usize, Option<[u8; 32]>)> {
    let mut shape = Vec::new();
    for contact in wb.storage().contacts().list_contacts().unwrap() {
        for update in wb
            .storage()
            .pending()
            .get_pending_updates(contact.id())
            .unwrap()
        {
            let message: RatchetMessage = serde_json::from_slice(&update.payload).unwrap();
            shape.push((
                update.update_type,
                message.ciphertext.len(),
                update.target_device_id,
            ));
        }
    }
    shape.sort();
    shape
}

fn edit_then_tick(world: &mut World, edits: &[Edit]) {
    for edit in edits {
        apply(&mut world.alice, edit);
    }
    world.alice.run_owed_repropagation().unwrap();
}

/// Bob receives everything Alice queued for him; the outcomes, in order.
fn bob_receives_all(world: &World) -> Vec<ReceiveOutcome> {
    world
        .alice
        .storage()
        .pending()
        .get_pending_updates(&world.bob_id)
        .unwrap()
        .iter()
        .map(|update| {
            process_single_card_update(
                world.bob.identity().unwrap(),
                world.bob.storage(),
                &world.alice_id,
                &update.payload,
            )
            .unwrap()
        })
        .collect()
}

fn work_email() -> Edit {
    Edit::AddPublicField {
        label: "work".into(),
        value: "alice@work.example".into(),
    }
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn a_duress_edit_leaves_the_queue_a_normal_edit_would() {
    let edits = [work_email(), Edit::Rename("Alicia".into())];
    let mut normal = normal_world();
    let mut duress = duress_world();

    edit_then_tick(&mut normal, &edits);
    edit_then_tick(&mut duress, &edits);

    let normal_shape = queue_shape(&normal.alice);
    assert_eq!(normal_shape.len(), 1, "the real edit reaches Bob once");
    assert_eq!(queue_shape(&duress.alice), normal_shape);
    assert_eq!(
        duress.alice.pending_update_count().unwrap(),
        normal.alice.pending_update_count().unwrap()
    );
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn bob_opens_a_duress_cover_as_nothing_and_a_real_edit_as_the_edit() {
    let edits = [work_email(), Edit::Rename("Alicia".into())];
    let mut normal = normal_world();
    let mut duress = duress_world();
    edit_then_tick(&mut normal, &edits);
    edit_then_tick(&mut duress, &edits);

    let outcomes = bob_receives_all(&normal);
    assert!(
        matches!(outcomes.as_slice(), [ReceiveOutcome::CardDelta]),
        "got {outcomes:?}"
    );
    let alice_at_bob = normal.bob.get_contact(&normal.alice_id).unwrap().unwrap();
    assert_eq!(alice_at_bob.card().display_name(), "Alicia");
    assert_eq!(alice_at_bob.card().fields().len(), 1);

    let outcomes = bob_receives_all(&duress);
    assert!(
        matches!(outcomes.as_slice(), [ReceiveOutcome::Cover]),
        "got {outcomes:?}"
    );
    let alice_at_bob = duress.bob.get_contact(&duress.alice_id).unwrap().unwrap();
    assert_eq!(alice_at_bob.card().display_name(), "Alice");
    assert!(alice_at_bob.card().fields().is_empty());
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn every_own_card_writer_under_duress_queues_only_cover() {
    let mut duress = duress_world();
    let field = ContactField::new(FieldType::Phone, "Phone", "+41 79 000 00 00", 0);
    let field_id = field.id().to_string();
    let wb = &duress.alice;

    wb.add_own_field(field).unwrap();
    wb.set_field_shown(&field_id, true).unwrap();
    wb.set_own_field_public(&field_id).unwrap();
    wb.set_own_field_private(&field_id).unwrap();
    let mut card = wb.own_card().unwrap().unwrap();
    card.set_bio("under duress").unwrap();
    wb.update_own_card(&card).unwrap();
    assert!(wb.remove_own_field_by_id(&field_id).unwrap());
    duress.alice.update_display_name("Alicia").unwrap();
    duress.alice.run_owed_repropagation().unwrap();

    let outcomes = bob_receives_all(&duress);
    assert!(
        matches!(outcomes.as_slice(), [ReceiveOutcome::Cover]),
        "got {outcomes:?}"
    );
    let alice_at_bob = duress.bob.get_contact(&duress.alice_id).unwrap().unwrap();
    assert_eq!(alice_at_bob.card().display_name(), "Alice");
    assert!(alice_at_bob.card().fields().is_empty());
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn a_duress_edit_that_changes_nothing_visible_queues_what_a_normal_one_would() {
    let edits = [Edit::AddHiddenField {
        label: "secret".into(),
        value: "hidden@example".into(),
    }];
    let mut normal = normal_world();
    let mut duress = duress_world();

    edit_then_tick(&mut normal, &edits);
    edit_then_tick(&mut duress, &edits);

    assert_eq!(queue_shape(&duress.alice), queue_shape(&normal.alice));
}

/// Labels carry `index` so a sequence never adds the same label twice.
/// Proptest's counterexample: the duress rename path returned before the
/// marker a real rename sets, so a rename alone queued no cover.
// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn a_rename_alone_under_duress_queues_what_a_normal_one_would() {
    let edits = [Edit::Rename("A".into())];
    let mut normal = normal_world();
    let mut duress = duress_world();

    edit_then_tick(&mut normal, &edits);
    edit_then_tick(&mut duress, &edits);

    let normal_shape = queue_shape(&normal.alice);
    assert_eq!(normal_shape.len(), 1, "the real rename reaches Bob once");
    assert_eq!(queue_shape(&duress.alice), normal_shape);
}

fn edit_strategy(index: usize) -> impl Strategy<Value = Edit> {
    let label = move |label: String| format!("{label}{index}");
    prop_oneof![
        ("[a-z]{1,12}", "[a-z0-9.]{1,20}@[a-z]{1,10}\\.[a-z]{2,4}").prop_map(move |(l, value)| {
            Edit::AddPublicField {
                label: label(l),
                value,
            }
        }),
        ("[a-z]{1,12}", "[a-z0-9 ]{1,40}").prop_map(move |(l, value)| Edit::AddHiddenField {
            label: label(l),
            value,
        }),
        "[A-Za-z]{1,20}".prop_map(Edit::Rename),
    ]
}

fn edits_strategy() -> impl Strategy<Value = Vec<Edit>> {
    (1usize..=3).prop_flat_map(|count| (0..count).map(edit_strategy).collect::<Vec<_>>())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    // @scenario: duress_mode :: Duress mode looks identical to normal mode
    #[test]
    fn any_duress_edit_sequence_leaves_the_queue_a_normal_one_would(edits in edits_strategy()) {
        let mut normal = normal_world();
        let mut duress = duress_world();

        edit_then_tick(&mut normal, &edits);
        edit_then_tick(&mut duress, &edits);

        prop_assert_eq!(queue_shape(&duress.alice), queue_shape(&normal.alice));
        prop_assert_eq!(
            duress.alice.pending_update_count().unwrap(),
            normal.alice.pending_update_count().unwrap()
        );
    }
}
