// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decommission contract for a replaced device: wiping every contact
//! ratchet session returns the retired device to the fail-safe path —
//! the send loop skips contacts without a session, so it can no longer
//! advance a chain the replacement device now owns.
//!
//! Related problem records:
//! - `2026-03-23-device-replacement-flow` (decommission step)
//! - `2026-07-10-multi-device-ratchet-topology-gap`

use crate::common::helpers::{create_vauchi_with_identity, setup_ratchets};
use vauchi_core::VauchiError;
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

fn add_contact_with_ratchet(vauchi: &Vauchi, name: &str, public_key: [u8; 32]) -> String {
    let contact = Contact::from_exchange(
        public_key,
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    let contact_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    let (ratchet, _) = setup_ratchets(&SymmetricKey::generate());
    vauchi.save_ratchet_state(&contact_id, &ratchet).unwrap();
    contact_id
}

// @internal
#[test]
fn decommission_wipes_all_contact_ratchet_sessions() {
    let alice = create_vauchi_with_identity("Alice");
    let bob_id = add_contact_with_ratchet(&alice, "Bob", [0x41u8; 32]);
    let carol_id = add_contact_with_ratchet(&alice, "Carol", [0x42u8; 32]);

    let wiped = alice.decommission_current_device().unwrap();

    assert_eq!(wiped, 2, "both sessions counted");
    assert!(
        alice.get_ratchet_state(&bob_id).unwrap().is_none(),
        "session for Bob must be gone"
    );
    assert!(
        alice.get_ratchet_state(&carol_id).unwrap().is_none(),
        "session for Carol must be gone"
    );
}

// @internal
#[test]
fn decommission_with_no_sessions_wipes_zero() {
    let alice = create_vauchi_with_identity("Alice");

    assert_eq!(alice.decommission_current_device().unwrap(), 0);
}

// @internal
#[test]
fn decommission_without_identity_errors() {
    let vauchi = Vauchi::in_memory().unwrap();

    let err = vauchi.decommission_current_device().unwrap_err();

    assert!(
        matches!(err, VauchiError::IdentityNotInitialized),
        "decommission on an uninitialized instance must refuse, got: {err:?}"
    );
}
