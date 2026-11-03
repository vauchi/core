// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Restoring the guardian backup into the new identity (vauchi/private#796
//! step 3, D6): with one returned share of every group the new phone opens
//! the backup its guardians handed back; Alice's contacts, labels and own
//! card come back under the new identity, each restored exchanged contact
//! waits for its rekey, and a guardian she just met again keeps the new
//! relationship.

use vauchi_core::Vauchi;

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::{deliver, meet, signing_pk};
use crate::common::two_recipient::{Recipient, add_recipient, receive};

const PASSPHRASE: &str = "the owl flies at noon";
const FAMILY_FIELD: &str = "home-phone";

struct World {
    guardians: Vec<Recipient>,
    dave_id: String,
    phone: Vauchi,
    /// Each guardian's contact id at the phone.
    guardians_at_phone: Vec<String>,
}

/// Alice has Dave in her "Family" label and three guardians, each in a
/// group of its own, holding her backup (sealed under `passphrase` when
/// given). Her new phone claims her old key; `handing_back` of the
/// guardians vouch, meet the phone and hand back.
fn world(passphrase: Option<&str>, handing_back: usize) -> World {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = signing_pk(&alice);
    let dave = add_recipient(&alice, &alice_pk, "Dave");
    let family = alice.create_group("Family").unwrap();
    alice
        .add_contact_to_group(family.id(), &dave.id_at_sharer)
        .unwrap();
    alice
        .set_group_field_visibility(family.id(), FAMILY_FIELD, true)
        .unwrap();
    let mut guardians: Vec<Recipient> = (0..3)
        .map(|index| add_recipient(&alice, &alice_pk, &format!("G{index}")))
        .collect();
    for guardian in &guardians {
        alice
            .verify_contact_fingerprint(&guardian.id_at_sharer)
            .unwrap();
        alice.toggle_recovery_trust(&guardian.id_at_sharer).unwrap();
    }
    if let Some(passphrase) = passphrase {
        alice.set_recovery_passphrase(passphrase).unwrap();
    }
    for guardian in &mut guardians {
        receive(&alice, guardian);
    }

    let phone = create_vauchi_with_identity("Alice, new phone");
    let claim = phone
        .create_recovery_claim_hex_b64(&hex::encode(alice_pk))
        .unwrap();
    let mut guardians_at_phone = Vec::new();
    for guardian in guardians.iter().take(handing_back) {
        let g = guardian.vauchi();
        g.create_voucher_from_claim_b64(&claim).unwrap();
        let (phone_at_g, g_at_phone) = meet(g, &phone, true);
        deliver(g, &phone_at_g, &phone, &g_at_phone);
        guardians_at_phone.push(g_at_phone);
    }
    World {
        guardians,
        dave_id: dave.id_at_sharer.clone(),
        phone,
        guardians_at_phone,
    }
}

// @internal
#[test]
fn the_new_identity_gets_back_the_contacts_labels_and_own_card_and_keeps_its_key() {
    let world = world(None, 3);
    let new_pk = signing_pk(&world.phone);

    let restored = world.phone.restore_from_returned_backups(None).unwrap();

    assert_eq!(restored, 1, "Dave; the guardians were met again");
    let dave = world.phone.get_contact(&world.dave_id).unwrap().unwrap();
    assert_eq!(dave.display_name(), "Dave");
    let family: Vec<_> = world
        .phone
        .list_groups()
        .unwrap()
        .into_iter()
        .filter(|group| group.name() == "Family")
        .collect();
    assert_eq!(family.len(), 1);
    assert!(family[0].contacts().contains(&world.dave_id));
    assert_eq!(
        world.phone.own_card().unwrap().unwrap().display_name(),
        "Alice"
    );
    assert_eq!(signing_pk(&world.phone), new_pk, "D6: a new identity");
}

// @internal
#[test]
fn a_guardian_met_again_keeps_the_new_relationship() {
    let world = world(None, 3);
    let met = world
        .phone
        .get_contact(&world.guardians_at_phone[0])
        .unwrap()
        .unwrap();

    world.phone.restore_from_returned_backups(None).unwrap();

    let after = world
        .phone
        .get_contact(&world.guardians_at_phone[0])
        .unwrap()
        .unwrap();
    assert_eq!(
        after.shared_key().map(|key| *key.as_bytes()),
        met.shared_key().map(|key| *key.as_bytes())
    );
    assert_eq!(after.display_name(), met.display_name());
}

// @internal
#[test]
fn each_restored_exchanged_contact_waits_for_its_rekey_and_a_guardian_met_again_does_not() {
    let world = world(None, 3);

    world.phone.restore_from_returned_backups(None).unwrap();

    assert_eq!(
        world.phone.rekey_pending_contacts().unwrap(),
        vec![world.dave_id.clone()]
    );
}

// @internal
#[test]
fn without_a_share_of_every_group_nothing_is_restored() {
    let world = world(None, 2);

    assert_eq!(world.phone.guardians_handed_back().unwrap(), 2);
    assert!(world.phone.restore_from_returned_backups(None).is_err());

    assert!(world.phone.get_contact(&world.dave_id).unwrap().is_none());
    assert!(world.phone.rekey_pending_contacts().unwrap().is_empty());
}

// @internal
#[test]
fn with_nothing_handed_back_nothing_is_restored() {
    let world = world(None, 0);

    assert_eq!(world.phone.guardians_handed_back().unwrap(), 0);
    assert!(!world.phone.returned_backup_needs_passphrase().unwrap());
    assert!(world.phone.restore_from_returned_backups(None).is_err());
}

// @internal
#[test]
fn a_backup_sealed_under_the_passphrase_restores_only_with_it() {
    let world = world(Some(PASSPHRASE), 3);

    assert!(world.phone.returned_backup_needs_passphrase().unwrap());
    assert!(world.phone.restore_from_returned_backups(None).is_err());
    assert!(
        world
            .phone
            .restore_from_returned_backups(Some("the owl flies at dusk"))
            .is_err()
    );
    assert!(world.phone.get_contact(&world.dave_id).unwrap().is_none());

    assert_eq!(
        world
            .phone
            .restore_from_returned_backups(Some(PASSPHRASE))
            .unwrap(),
        1
    );
}

// @internal
#[test]
fn restoring_again_restores_nothing_new() {
    let world = world(None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();

    assert_eq!(world.phone.restore_from_returned_backups(None).unwrap(), 0);
    assert_eq!(
        world.phone.rekey_pending_contacts().unwrap(),
        vec![world.dave_id.clone()]
    );
    assert_eq!(world.guardians.len(), 3);
}

// @internal
#[test]
fn a_restored_label_shows_the_fields_it_showed() {
    let world = world(None, 3);

    world.phone.restore_from_returned_backups(None).unwrap();

    let family = world
        .phone
        .list_groups()
        .unwrap()
        .into_iter()
        .find(|group| group.name() == "Family")
        .unwrap();
    assert_eq!(
        family.visible_fields().iter().collect::<Vec<_>>(),
        vec![FAMILY_FIELD]
    );
}
