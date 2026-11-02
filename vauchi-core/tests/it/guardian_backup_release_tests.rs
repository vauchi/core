// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A guardian taken out of the set forgets what it held for the designator,
//! told by a bodyless `0x0c` message (vauchi/private#796 step 1d'). The
//! ratchet authenticates the designator, and a release only reaches what
//! its own sender left.

use vauchi_core::api::ReceiveOutcome;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::sync::delta::{
    PAYLOAD_VERSION_GUARDIAN_RELEASE, VersionedPayload, encode_guardian_release,
};
use vauchi_core::sync::guardian_piece::{GuardianDelivery, split_into_pieces};
use vauchi_core::{SymmetricKey, Vauchi};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::two_recipient::{Recipient, add_recipient, receive};

fn kept(guardian: &Recipient) -> Option<GuardianDelivery> {
    guardian
        .vauchi()
        .storage()
        .guardian_backups()
        .kept(guardian.sharer_id())
        .unwrap()
}

fn alice_trusting(count: usize) -> (Vauchi, Vec<Recipient>) {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let guardians: Vec<Recipient> = (0..count)
        .map(|index| add_recipient(&alice, &alice_pk, &format!("G{index}")))
        .collect();
    for guardian in &guardians {
        alice
            .verify_contact_fingerprint(&guardian.id_at_sharer)
            .unwrap();
        assert!(alice.toggle_recovery_trust(&guardian.id_at_sharer).unwrap());
    }
    (alice, guardians)
}

// @internal
#[test]
fn a_guardian_release_is_its_own_bodyless_message_kind() {
    assert_eq!(PAYLOAD_VERSION_GUARDIAN_RELEASE, 0x0c);
    assert_eq!(encode_guardian_release(), vec![0x0c]);
    assert!(matches!(
        VersionedPayload::decode(&encode_guardian_release()),
        Ok(VersionedPayload::GuardianRelease)
    ));
}

// @internal
#[test]
fn a_guardian_taken_out_of_the_set_forgets_the_backup_and_the_rest_keep_the_new_one() {
    let (alice, mut guardians) = alice_trusting(4);
    for guardian in &mut guardians {
        receive(&alice, guardian);
    }
    let before = kept(&guardians[0])
        .expect("a kept backup")
        .backup()
        .to_vec();

    assert!(
        !alice
            .toggle_recovery_trust(&guardians[3].id_at_sharer)
            .unwrap()
    );

    let released: Vec<ReceiveOutcome> = receive(&alice, &mut guardians[3]);
    assert_eq!(released.len(), 1, "the release, and no new backup");
    assert!(matches!(released[0], ReceiveOutcome::GuardianRelease));
    assert_eq!(kept(&guardians[3]), None);
    let after: Vec<Vec<u8>> = guardians[..3]
        .iter_mut()
        .map(|guardian| {
            receive(&alice, guardian);
            kept(guardian).expect("a kept backup").backup().to_vec()
        })
        .collect();
    assert!(after.iter().all(|backup| *backup == after[0]));
    assert_ne!(after[0], before, "the three keep the new split");
}

// @internal
#[test]
fn a_release_forgets_only_what_its_sender_left() {
    let guardian = create_vauchi_with_identity("Guardian");
    let ids: Vec<String> = ["Alice", "Bob"]
        .iter()
        .map(|name| {
            let pk = *SigningKeyPair::generate().public_key().as_bytes();
            let contact =
                Contact::from_exchange(pk, ContactCard::new(name), SymmetricKey::generate(), 0);
            guardian
                .storage()
                .contacts()
                .save_contact(&contact)
                .unwrap();
            contact.id().to_string()
        })
        .collect();
    let store = guardian.storage().guardian_backups();
    for (fill, id) in ids.iter().enumerate() {
        let delivery =
            GuardianDelivery::new(1, vec![fill as u8 + 1; 124], vec![fill as u8 + 1; 10]).unwrap();
        for piece in split_into_pieces([fill as u8; 16], &delivery.encode()).unwrap() {
            store.keep_piece(id, &piece, 1).unwrap();
        }
    }

    store.forget_designator(&ids[1]).unwrap();

    assert_eq!(store.kept(&ids[1]).unwrap(), None);
    assert_eq!(
        store.kept(&ids[0]).unwrap().map(|d| d.backup().to_vec()),
        Some(vec![1; 10])
    );
}
