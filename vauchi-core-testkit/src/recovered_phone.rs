// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Alice's new phone after her guardians vouched and handed her backup
//! back (vauchi/private#796): the world the restore and rekey tests share.

use std::sync::Arc;
use std::time::SystemTime;

use vauchi_core::api::RekeyOfferTaken;
use vauchi_core::clock::FakeClock;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::recovery::authorization::RecoveryAuthorization;
use vauchi_core::recovery::rekey::RekeyOffer;
use vauchi_core::{SymmetricKey, Vauchi};

use crate::helpers::create_vauchi_with_identity;
use crate::meeting::{deliver, meet, signing_pk};
use crate::two_recipient::{Recipient, add_recipient, add_recipient_from, receive};

pub const PASSPHRASE: &str = "the owl flies at noon";
pub const FAMILY_FIELD: &str = "home-phone";

pub struct World {
    /// Alice's old device, which the recovery replaces.
    pub alice: Vauchi,
    pub guardians: Vec<Recipient>,
    pub dave: Recipient,
    pub dave_id: String,
    /// Dave's clock, which a test may move forward.
    pub dave_clock: Arc<FakeClock>,
    pub phone: Vauchi,
    /// Each guardian's contact id at the phone.
    pub guardians_at_phone: Vec<String>,
}

impl World {
    /// Dave takes the phone's rekey offer as if the recovery were
    /// authorized: what follows an authorization, without the decision
    /// that grants one, which nothing grants yet (P18).
    pub fn dave_takes_as_authorized(&self, offer: &[u8]) -> RekeyOfferTaken {
        let new_pk = *RekeyOffer::decode(offer).unwrap().new_pk();
        let authorization =
            RecoveryAuthorization::assumed_for_tests(signing_pk(&self.alice), new_pk);
        self.dave
            .vauchi()
            .take_rekey_offer_authorized(offer, &authorization)
            .unwrap()
    }
}

/// Dave no longer knows Alice's guardians: their vouchers are then nobody
/// he can place, as a thief's sock puppets would be.
pub fn dave_forgets_the_guardians(world: &World) {
    for guardian in &world.guardians {
        world
            .dave
            .vauchi()
            .remove_contact(&hex::encode(signing_pk(guardian.vauchi())))
            .unwrap();
    }
}

/// Alice has Dave in her "Family" label and three guardians, each in a
/// group of its own, holding her backup (sealed under `passphrase` when
/// given). Her new `phone` claims her old key; `handing_back` of the
/// guardians vouch (the phone takes in each voucher), meet the phone and
/// hand back.
pub fn recovered(phone: Vauchi, passphrase: Option<&str>, handing_back: usize) -> World {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = signing_pk(&alice);
    let dave_clock = Arc::new(FakeClock::new(SystemTime::now()));
    let mut dave_vauchi = Vauchi::in_memory_with_clock(dave_clock.clone()).unwrap();
    dave_vauchi.create_identity("Dave").unwrap();
    let dave = add_recipient_from(&alice, &alice_pk, dave_vauchi);
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
    // Dave knows Alice's guardians himself, so their vouchers count for him
    // on keys a thief with the old device does not hold (D18's contacts'
    // route; the security review of 2026-11-03).
    for guardian in &guardians {
        dave.vauchi()
            .add_contact(Contact::from_exchange(
                signing_pk(guardian.vauchi()),
                ContactCard::new("Guardian"),
                SymmetricKey::generate(),
                0,
            ))
            .unwrap();
    }

    let claim = phone
        .create_recovery_claim_hex_b64(&hex::encode(alice_pk))
        .unwrap();
    let mut guardians_at_phone = Vec::new();
    for guardian in guardians.iter().take(handing_back) {
        let g = guardian.vauchi();
        let voucher = g.create_voucher_from_claim_b64(&claim).unwrap();
        phone.add_recovery_voucher_b64(&voucher).unwrap();
        let (phone_at_g, g_at_phone) = meet(g, &phone, true);
        deliver(g, &phone_at_g, &phone, &g_at_phone);
        guardians_at_phone.push(g_at_phone);
    }
    World {
        alice,
        guardians,
        dave_id: dave.id_at_sharer.clone(),
        dave_clock,
        dave,
        phone,
        guardians_at_phone,
    }
}
