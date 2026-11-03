// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The owner puts each guardian in a group as it is added, and the backup
//! is split over the groups: one Shamir share per group, sealed to every
//! member, opened by one guardian of every group (vauchi/private#796, D17:
//! 3 <= groups <= guardians <= 9, none empty). Fewer than three groups make
//! no backup.

use std::collections::HashMap;

use vauchi_core::Vauchi;
use vauchi_core::api::ReceiveOutcome;
use vauchi_core::sync::guardian_piece::{GuardianDelivery, PieceAssembly};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::two_recipient::{Recipient, add_recipient, receive};

/// Alice and `count` exchanged, fingerprint-verified contacts, none trusted
/// for recovery yet.
fn alice_with_contacts(count: usize) -> (Vauchi, Vec<Recipient>) {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let contacts: Vec<Recipient> = (0..count)
        .map(|index| add_recipient(&alice, &alice_pk, &format!("G{index}")))
        .collect();
    for contact in &contacts {
        alice
            .verify_contact_fingerprint(&contact.id_at_sharer)
            .unwrap();
    }
    (alice, contacts)
}

/// Trusts every contact in the group the shape gives it: `shape[i]` is the
/// number of members of the i-th group, filled in order. Returns each
/// contact's group.
fn group(alice: &Vauchi, contacts: &[Recipient], shape: &[usize]) -> Vec<u8> {
    let mut groups = Vec::new();
    let mut next = contacts.iter();
    for &size in shape {
        let first = next.next().unwrap();
        let id = alice
            .trust_for_recovery_in_group(&first.id_at_sharer, None)
            .unwrap();
        groups.push(id);
        for _ in 1..size {
            let member = next.next().unwrap();
            assert_eq!(
                alice
                    .trust_for_recovery_in_group(&member.id_at_sharer, Some(id))
                    .unwrap(),
                id
            );
            groups.push(id);
        }
    }
    groups
}

/// The newest whole delivery each guardian received.
fn deliveries(alice: &Vauchi, contacts: &mut [Recipient]) -> Vec<Option<GuardianDelivery>> {
    contacts
        .iter_mut()
        .map(|contact| {
            let mut assemblies: HashMap<[u8; 16], PieceAssembly> = HashMap::new();
            let mut whole = None;
            for outcome in receive(alice, contact) {
                if let ReceiveOutcome::GuardianBackupPiece(piece) = outcome
                    && let Some(bytes) = assemblies
                        .entry(piece.backup_id())
                        .or_default()
                        .add(piece)
                        .unwrap()
                {
                    whole = Some(GuardianDelivery::decode(&bytes).unwrap());
                }
            }
            whole
        })
        .collect()
}

/// Whether the shares of `holders` (by index), re-sealed to a new
/// identity, open the backup as Alice's.
fn opens(
    contacts: &[Recipient],
    delivered: &[Option<GuardianDelivery>],
    holders: &[usize],
) -> bool {
    let recovering = create_vauchi_with_identity("Alice, new phone");
    let new_pk = *recovering.identity().unwrap().signing_public_key();
    let backup = delivered[holders[0]].as_ref().unwrap().backup().to_vec();
    let shares: Vec<Vec<u8>> = holders
        .iter()
        .map(|&index| {
            contacts[index]
                .vauchi()
                .respond_to_recovery(delivered[index].as_ref().unwrap().sealed_share(), &new_pk)
                .unwrap()
        })
        .collect();
    recovering
        .recover_guardian_backup(&hex::encode(backup), &shares)
        .is_ok_and(|envelope| envelope.sections.identity.display_name == "Alice")
}

// @internal
#[test]
fn the_owner_puts_a_guardian_in_a_new_group_or_an_existing_one_as_it_is_added() {
    let (alice, contacts) = alice_with_contacts(5);

    let groups = group(&alice, &contacts, &[2, 2, 1]);

    assert_eq!(groups[0], groups[1]);
    assert_eq!(groups[2], groups[3]);
    assert_ne!(groups[0], groups[2]);
    assert_ne!(groups[2], groups[4]);
    let listed = alice.guardian_groups().unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|(_, members)| members.len())
            .collect::<Vec<_>>(),
        [2, 2, 1]
    );
}

// @internal
#[test]
fn one_guardian_of_every_group_opens_and_all_groups_but_one_do_not() {
    let (alice, mut contacts) = alice_with_contacts(5);
    group(&alice, &contacts, &[2, 2, 1]);

    let delivered = deliveries(&alice, &mut contacts);

    assert!(delivered.iter().all(Option::is_some));
    assert!(opens(&contacts, &delivered, &[1, 2, 4]));
    assert!(opens(&contacts, &delivered, &[0, 3, 4]));
    assert!(
        !opens(&contacts, &delivered, &[0, 1, 2, 3]),
        "group three missing"
    );
}

// @internal
#[test]
fn nine_guardians_as_four_two_two_one_is_a_valid_split() {
    let (alice, mut contacts) = alice_with_contacts(9);
    group(&alice, &contacts, &[4, 2, 2, 1]);

    let delivered = deliveries(&alice, &mut contacts);

    assert!(opens(&contacts, &delivered, &[3, 5, 6, 8]));
    assert!(
        !opens(&contacts, &delivered, &[0, 1, 4, 6]),
        "group four missing"
    );
}

// @internal
#[test]
fn fewer_than_three_groups_make_no_backup_however_many_guardians() {
    let (alice, mut contacts) = alice_with_contacts(4);
    group(&alice, &contacts, &[2, 2]);

    let delivered = deliveries(&alice, &mut contacts);

    assert!(delivered.iter().all(Option::is_none));
    assert_eq!(alice.send_guardian_backup().unwrap(), 0);
}

// @internal
#[test]
fn a_group_number_that_does_not_exist_is_refused() {
    let (alice, contacts) = alice_with_contacts(1);

    assert!(
        alice
            .trust_for_recovery_in_group(&contacts[0].id_at_sharer, Some(7))
            .is_err()
    );
    assert!(alice.guardian_groups().unwrap().is_empty());
}
