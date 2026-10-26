// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a duress session does to the own card stays in a copy of its own:
//! it reaches no contact, the real card is untouched, and the copy survives
//! a lock and a second duress unlock, as a real edit would (ADR-032,
//! vauchi/private#811).

use vauchi_core::api::AuthMode;
use vauchi_core::{Contact, ContactCard, ContactField, FieldType, Vauchi};

use crate::common::helpers::create_vauchi_with_identity;

const APP_PASSWORD: &str = "my-pin-1234";
const DURESS_PIN: &str = "654321";

/// Alice with one ratcheted contact, unlocked with the duress PIN.
fn alice_under_duress() -> Vauchi {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password(APP_PASSWORD).unwrap();
    wb.setup_duress_password(DURESS_PIN).unwrap();
    let friend = Contact::from_exchange(
        [7; 32],
        ContactCard::new("Trusted Friend"),
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    let friend_id = friend.id().to_string();
    wb.add_contact(friend).unwrap();
    let dh = vauchi_core::exchange::X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(
        &friend_id,
        &vauchi_core::crypto::SymmetricKey::generate(),
        *dh.public_key(),
    )
    .unwrap();
    assert_eq!(wb.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);
    wb
}

fn phone_field() -> ContactField {
    ContactField::new(FieldType::Phone, "Phone", "+41 79 000 00 00", 0)
}

fn has_phone(card: &ContactCard) -> bool {
    card.fields().iter().any(|f| f.label() == "Phone")
}

fn pending_updates(wb: &Vauchi) -> usize {
    wb.storage().pending().count_all_pending_updates().unwrap()
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn a_duress_edit_survives_a_lock_and_a_second_duress_unlock() {
    let mut wb = alice_under_duress();
    wb.add_own_field(phone_field()).unwrap();

    assert_eq!(wb.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);

    let card = wb.own_card().unwrap().expect("duress copy");
    assert!(
        has_phone(&card),
        "an edit that vanished would give duress mode away"
    );
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn a_duress_edit_leaves_the_real_card_untouched() {
    let mut wb = alice_under_duress();
    wb.add_own_field(phone_field()).unwrap();
    wb.update_display_name("Mallory").unwrap();
    assert_eq!(
        wb.own_card().unwrap().unwrap().display_name(),
        "Mallory",
        "the duress session must see its own rename"
    );

    assert_eq!(wb.authenticate(APP_PASSWORD).unwrap(), AuthMode::Normal);

    let real = wb.own_card().unwrap().expect("real card");
    assert!(!has_phone(&real));
    assert_eq!(real.display_name(), "Alice");
    assert_eq!(wb.identity().unwrap().display_name(), "Alice");
}

/// Every own-card writer a screen reaches: none queues anything for a
/// real contact.
// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn disabling_duress_discards_the_copy() {
    let mut wb = alice_under_duress();
    wb.add_own_field(phone_field()).unwrap();
    assert_eq!(wb.authenticate(APP_PASSWORD).unwrap(), AuthMode::Normal);

    wb.disable_duress().unwrap();
    wb.setup_duress_password(DURESS_PIN).unwrap();
    assert_eq!(wb.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);

    let card = wb
        .own_card()
        .unwrap()
        .expect("a fresh copy of the real card");
    assert!(!has_phone(&card));
}
