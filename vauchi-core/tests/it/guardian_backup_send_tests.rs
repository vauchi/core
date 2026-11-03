// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Designating guardians sends each one the guardian backup with its own
//! sealed share, in mailbox pieces (vauchi/private#796, D7, D9). The shards
//! open at the voucher threshold, capped at the guardian count and never
//! below Shamir's two (D10); fewer than three guardians make no backup.

use std::collections::HashMap;

use vauchi_core::api::ReceiveOutcome;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::sync::guardian_piece::{GuardianDelivery, PieceAssembly, PieceError};
use vauchi_core::{SymmetricKey, Vauchi, VauchiError};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::two_recipient::{Recipient, add_recipient, receive};

fn alice_with_guardians(count: usize) -> (Vauchi, Vec<Recipient>) {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let guardians = (0..count)
        .map(|index| add_recipient(&alice, &alice_pk, &format!("G{index}")))
        .collect();
    (alice, guardians)
}

fn trust_all(alice: &Vauchi, guardians: &[Recipient]) {
    for guardian in guardians {
        alice
            .verify_contact_fingerprint(&guardian.id_at_sharer)
            .unwrap();
        assert!(alice.toggle_recovery_trust(&guardian.id_at_sharer).unwrap());
    }
}

/// The newest whole delivery among the pieces `guardian` received.
fn latest_delivery(alice: &Vauchi, guardian: &mut Recipient) -> Option<GuardianDelivery> {
    let mut assemblies: HashMap<[u8; 16], PieceAssembly> = HashMap::new();
    let mut whole = None;
    for outcome in receive(alice, guardian) {
        let ReceiveOutcome::GuardianBackupPiece(piece) = outcome else {
            continue;
        };
        let assembly = assemblies.entry(piece.backup_id()).or_default();
        if let Some(bytes) = assembly.add(piece).unwrap() {
            whole = Some(GuardianDelivery::decode(&bytes).unwrap());
        }
    }
    whole
}

/// Whether the shares these guardians hold, re-sealed to a new identity,
/// open `backup` as Alice's.
fn opens(backup: &[u8], holders: &[(&Recipient, &GuardianDelivery)]) -> bool {
    let recovering = create_vauchi_with_identity("Alice, new phone");
    let new_pk = *recovering.identity().unwrap().signing_public_key();
    let resealed: Vec<Vec<u8>> = holders
        .iter()
        .map(|(guardian, delivery)| {
            guardian
                .vauchi()
                .respond_to_recovery(delivery.sealed_share(), &new_pk)
                .unwrap()
        })
        .collect();
    recovering
        .recover_guardian_backup(&hex::encode(backup), &resealed)
        .is_ok_and(|envelope| envelope.sections.identity.display_name == "Alice")
}

// @internal
#[test]
fn a_guardian_delivery_carries_its_time_share_and_backup_and_decodes_back() {
    let delivery = GuardianDelivery::new(1_700_000_000, vec![3; 124], vec![9; 5000]).unwrap();

    let decoded = GuardianDelivery::decode(&delivery.encode()).unwrap();

    assert_eq!(decoded.created_at(), 1_700_000_000);
    assert_eq!(decoded.sealed_share(), &[3; 124]);
    assert_eq!(decoded.backup(), &[9; 5000]);
}

/// The wire form is the creation time (8 bytes), the share length
/// (2 bytes), the share and the backup.
// @internal
#[test]
fn a_malformed_guardian_delivery_is_refused() {
    let encoded = GuardianDelivery::new(7, vec![3; 124], vec![9; 10])
        .unwrap()
        .encode();
    let mut long_share = encoded.clone();
    long_share[8..10].copy_from_slice(&2000u16.to_be_bytes());
    let mut no_share = vec![0; 8];
    no_share.extend_from_slice(&[0, 0, 9]);

    let cases: [(&str, Vec<u8>); 6] = [
        ("empty", Vec::new()),
        ("only part of the time", encoded[..5].to_vec()),
        ("no share", no_share),
        ("share past the end", long_share),
        ("cut inside the share", encoded[..68].to_vec()),
        ("no backup", encoded[..134].to_vec()),
    ];
    for (name, bytes) in cases {
        assert_eq!(
            GuardianDelivery::decode(&bytes).err(),
            Some(PieceError::Malformed),
            "{name}"
        );
    }
    assert_eq!(
        GuardianDelivery::new(7, Vec::new(), vec![1]).err(),
        Some(PieceError::Malformed)
    );
}

// @internal
#[test]
fn designating_three_guardians_sends_each_a_backup_all_three_open_and_two_do_not() {
    let (alice, mut guardians) = alice_with_guardians(3);
    trust_all(&alice, &guardians);

    let deliveries: Vec<GuardianDelivery> = guardians
        .iter_mut()
        .map(|guardian| latest_delivery(&alice, guardian).expect("a whole backup"))
        .collect();
    let backup = deliveries[0].backup().to_vec();
    assert!(deliveries.iter().all(|d| d.backup() == backup));
    let holders: Vec<(&Recipient, &GuardianDelivery)> = guardians.iter().zip(&deliveries).collect();

    assert!(opens(&backup, &holders));
    assert!(!opens(&backup, &holders[..2]));
}

// @internal
#[test]
fn fewer_than_three_guardians_get_no_backup() {
    let (alice, mut guardians) = alice_with_guardians(2);
    trust_all(&alice, &guardians);

    assert_eq!(alice.send_guardian_backup().unwrap(), 0);
    for guardian in &mut guardians {
        assert!(latest_delivery(&alice, guardian).is_none());
    }
}

// @internal
#[test]
fn a_removed_guardians_share_does_not_open_the_next_backup() {
    let (alice, mut guardians) = alice_with_guardians(4);
    trust_all(&alice, &guardians);
    let removed_share = latest_delivery(&alice, &mut guardians[3]).unwrap();

    assert!(
        !alice
            .toggle_recovery_trust(&guardians[3].id_at_sharer)
            .unwrap()
    );

    let kept: Vec<GuardianDelivery> = guardians[..3]
        .iter_mut()
        .map(|guardian| latest_delivery(&alice, guardian).unwrap())
        .collect();
    let backup = kept[0].backup().to_vec();
    assert!(latest_delivery(&alice, &mut guardians[3]).is_none());
    assert!(!opens(
        &backup,
        &[
            (&guardians[0], &kept[0]),
            (&guardians[1], &kept[1]),
            (&guardians[3], &removed_share)
        ]
    ));
    assert!(opens(
        &backup,
        &[
            (&guardians[0], &kept[0]),
            (&guardians[1], &kept[1]),
            (&guardians[2], &kept[2])
        ]
    ));
}

/// The trust change and the backup it re-splits are one change: when the
/// backup cannot be sent, the toggle fails and the guardian set stays.
// @internal
#[test]
fn a_trust_change_whose_backup_cannot_be_sent_fails_and_changes_nothing() {
    let wb = Vauchi::in_memory().unwrap();
    let pk = *SigningKeyPair::generate().public_key().as_bytes();
    let contact = Contact::from_exchange(pk, ContactCard::new("G"), SymmetricKey::generate(), 0);
    let id = contact.id().to_string();
    wb.storage().contacts().save_contact(&contact).unwrap();
    wb.verify_contact_fingerprint(&id).unwrap();

    let toggled = wb.toggle_recovery_trust(&id);

    assert!(
        matches!(toggled, Err(VauchiError::IdentityNotInitialized)),
        "got {toggled:?}"
    );
    let stored = wb.storage().contacts().load_contact(&id).unwrap().unwrap();
    assert!(!stored.is_recovery_trusted());
}
