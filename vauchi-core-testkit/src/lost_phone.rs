// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! An owner who lost her only phone, with a contact and three guardians
//! holding her backup, for the same-identity restore (vauchi/private#796,
//! P32).

use std::sync::Arc;

use vauchi_core::clock::Clock;
use vauchi_core::{Contact, ContactCard, Identity, SymmetricKey, Vauchi};

use super::helpers::create_vauchi_with_identity;
use super::meeting::signing_pk;
use super::two_recipient::{Recipient, add_recipient, add_recipient_from, receive};

/// Alice with Dave as a contact and three guardians, each in a group of
/// its own, holding her backup.
pub struct Lost {
    pub alice: Vauchi,
    pub dave: Recipient,
    pub guardians: Vec<Recipient>,
}

pub fn lost() -> Lost {
    lost_with_extra_contacts(0)
}

/// As [`lost`], with `extra` more contacts, so the backup can outgrow one
/// piece.
pub fn lost_with_extra_contacts(extra: usize) -> Lost {
    let alice = create_vauchi_with_identity("Alice");
    add_extra_contacts(&alice, extra);
    let alice_pk = signing_pk(&alice);
    let guardians = (0..3)
        .map(|index| add_recipient(&alice, &alice_pk, &format!("G{index}")))
        .collect();
    finish_lost(alice, guardians)
}

/// As [`lost`], with the guardians on `clock`, for tests of their timers.
pub fn lost_with_guardians_on(clock: &Arc<dyn Clock>) -> Lost {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = signing_pk(&alice);
    let guardians = (0..3)
        .map(|index| {
            let mut guardian = Vauchi::in_memory_with_clock(Arc::clone(clock)).unwrap();
            guardian.create_identity(&format!("G{index}")).unwrap();
            add_recipient_from(&alice, &alice_pk, guardian)
        })
        .collect();
    finish_lost(alice, guardians)
}

fn add_extra_contacts(alice: &Vauchi, extra: usize) {
    for index in 0..extra {
        let contact = Contact::from_exchange(
            *Identity::create(&format!("C{index}"), 0).signing_public_key(),
            ContactCard::new(&format!("Contact with a long enough name {index}")),
            SymmetricKey::generate(),
            0,
        );
        alice.add_contact(contact).unwrap();
    }
}

fn finish_lost(alice: Vauchi, mut guardians: Vec<Recipient>) -> Lost {
    let alice_pk = signing_pk(&alice);
    let dave = add_recipient_from(&alice, &alice_pk, create_vauchi_with_identity("Dave"));
    for guardian in &guardians {
        alice
            .verify_contact_fingerprint(&guardian.id_at_sharer)
            .unwrap();
        alice.toggle_recovery_trust(&guardian.id_at_sharer).unwrap();
    }
    for guardian in &mut guardians {
        receive(&alice, guardian);
    }
    Lost {
        alice,
        dave,
        guardians,
    }
}

pub fn alice_pk_hex(lost: &Lost) -> String {
    hex::encode(signing_pk(&lost.alice))
}

/// One guardian's hand-back in the meeting: the phone scans its share and
/// takes every piece of its backup.
pub fn hand_over(guardian: &Vauchi, claim: &str, phone: &Vauchi) {
    let hand_back = guardian.hand_back_in_person(claim).unwrap();
    phone.take_hand_back(hand_back.share()).unwrap();
    for piece in hand_back.backup_pieces() {
        phone.take_hand_back_piece(piece).unwrap();
    }
}
