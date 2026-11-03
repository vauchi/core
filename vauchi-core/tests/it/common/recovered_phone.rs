// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Alice's new phone after her guardians vouched and handed her backup
//! back (vauchi/private#796): the world the restore and rekey tests share.

use std::sync::Arc;
use std::time::SystemTime;

use vauchi_core::Vauchi;
use vauchi_core::clock::FakeClock;

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::{deliver, meet, signing_pk};
use crate::common::two_recipient::{Recipient, add_recipient, add_recipient_from, receive};

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
