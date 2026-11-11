// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A recovering phone meeting a guardian in person: the exchange through
//! the production seam, and delivering what one side queued to the other
//! (vauchi/private#796).

use vauchi_core::api::{ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::{SymmetricKey, Vauchi};

pub fn signing_pk(wb: &Vauchi) -> [u8; 32] {
    *wb.identity().unwrap().signing_public_key()
}

/// An in-person exchange through the production seam,
/// `save_exchanged_contact`, with `guardian_initiates` choosing the roles.
/// Returns the phone's id at the guardian and the guardian's at the phone.
pub fn meet(guardian: &Vauchi, phone: &Vauchi, guardian_initiates: bool) -> (String, String) {
    let shared = SymmetricKey::generate();
    let phone_at_g = Contact::from_exchange(
        signing_pk(phone),
        ContactCard::new("Phone"),
        shared.clone(),
        0,
    );
    let g_at_phone = Contact::from_exchange(
        signing_pk(guardian),
        ContactCard::new("G"),
        shared.clone(),
        0,
    );
    let responder_dh = X3DHKeyPair::generate();
    let initiator =
        DoubleRatchetState::initialize_initiator(&shared, *responder_dh.public_key()).unwrap();
    let responder = DoubleRatchetState::initialize_responder(&shared, responder_dh);
    let (g_ratchet, phone_ratchet) = if guardian_initiates {
        (initiator, responder)
    } else {
        (responder, initiator)
    };
    phone
        .save_exchanged_contact(&g_at_phone, &phone_ratchet, !guardian_initiates)
        .unwrap();
    guardian
        .save_exchanged_contact(&phone_at_g, &g_ratchet, guardian_initiates)
        .unwrap();
    (phone_at_g.id().to_string(), g_at_phone.id().to_string())
}

/// Delivers what `from` queued for `to_id` into `to` (from `from_id`) and
/// returns what `to` made of it.
pub fn deliver(from: &Vauchi, to_id: &str, to: &Vauchi, from_id: &str) -> Vec<ReceiveOutcome> {
    let pending = from.storage().pending().get_pending_updates(to_id).unwrap();
    for update in &pending {
        from.storage()
            .pending()
            .mark_update_sent(&update.id)
            .unwrap();
    }
    pending
        .iter()
        .map(|update| {
            process_single_card_update(
                to.identity().unwrap(),
                to.storage(),
                from_id,
                &update.payload,
            )
            .unwrap()
        })
        .collect()
}
