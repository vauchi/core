// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Nine guardians fit; a tenth is refused where the person can see it,
//! instead of the whole set silently failing to reach the relay, and a
//! guardian count is never cast through `u8` (vauchi/private#929).

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::{SymmetricKey, Vauchi, VauchiError};

use crate::common::helpers::create_vauchi_with_identity;

const MAX_GUARDIANS: usize = 9;

fn contact_of(vauchi: &Vauchi, name: &str) -> String {
    let pk = *SigningKeyPair::generate().public_key().as_bytes();
    let contact = Contact::from_exchange(pk, ContactCard::new(name), SymmetricKey::generate(), 0);
    vauchi.storage().contacts().save_contact(&contact).unwrap();
    vauchi.verify_contact_fingerprint(contact.id()).unwrap();
    contact.id().to_string()
}

fn recovery_trusted_count(vauchi: &Vauchi) -> usize {
    vauchi
        .list_contacts()
        .unwrap()
        .iter()
        .filter(|c| c.is_recovery_trusted())
        .count()
}

// @scenario: contact_recovery :: A tenth recovery-trusted contact is refused, not dropped
#[test]
fn a_tenth_recovery_trusted_contact_is_refused_not_dropped() {
    let alice = create_vauchi_with_identity("Alice");
    let ids: Vec<String> = (0..=MAX_GUARDIANS)
        .map(|n| contact_of(&alice, &format!("Helper {n}")))
        .collect();
    for id in &ids[..MAX_GUARDIANS] {
        assert_eq!(alice.toggle_recovery_trust(id).unwrap(), true);
    }

    let tenth = alice.toggle_recovery_trust(&ids[MAX_GUARDIANS]);

    match tenth {
        Err(VauchiError::InvalidState(message)) => assert!(
            message.contains("9"),
            "the refusal names the limit: {message}"
        ),
        other => panic!("a tenth guardian must be refused, got {other:?}"),
    }
    assert_eq!(recovery_trusted_count(&alice), MAX_GUARDIANS);
    assert!(
        !alice
            .get_contact(&ids[MAX_GUARDIANS])
            .unwrap()
            .unwrap()
            .is_recovery_trusted()
    );
    assert_eq!(
        alice.toggle_recovery_trust(&ids[0]).unwrap(),
        false,
        "a held guardian can still be released"
    );
    assert_eq!(
        alice.toggle_recovery_trust(&ids[MAX_GUARDIANS]).unwrap(),
        true,
        "the freed place can be taken"
    );
}

// @scenario: contact_recovery :: A tenth recovery-trusted contact is refused, not dropped
#[test]
fn a_guardian_count_past_one_byte_is_refused_not_wrapped() {
    let alice = create_vauchi_with_identity("Alice");
    let wraps_to_three = 256 + 3;
    let pks: Vec<[u8; 32]> = (0..wraps_to_three)
        .map(|_| *SigningKeyPair::generate().public_key().as_bytes())
        .collect();

    let too_many = alice.export_guardian_backup_with_shards(&pks, 3);
    let ten = alice.export_guardian_backup_with_shards(&pks[..MAX_GUARDIANS + 1], 3);

    assert!(
        matches!(too_many, Err(VauchiError::InvalidState(_))),
        "259 guardians must not become 3 shares, got {:?}",
        too_many.map(|(_, shares)| shares.len())
    );
    assert!(matches!(ten, Err(VauchiError::InvalidState(_))));
}
