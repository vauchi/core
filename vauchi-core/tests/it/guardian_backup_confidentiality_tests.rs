// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A guardian can never read the backup it keeps (vauchi/private#796): the
//! backup is encrypted under a key split by Shamir, each guardian holds one
//! share sealed to itself, and opening needs at least two shares (D10), so
//! everything one guardian holds opens nothing, even at the lowest voucher
//! threshold.

use vauchi_core::Vauchi;
use vauchi_core::recovery::RecoverySettings;
use vauchi_core::sync::guardian_piece::GuardianDelivery;

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::two_recipient::{Recipient, add_recipient, receive};

fn signing_pk(wb: &Vauchi) -> [u8; 32] {
    *wb.identity().unwrap().signing_public_key()
}

// @internal
#[test]
fn everything_one_guardian_holds_opens_nothing_even_at_the_lowest_threshold() {
    let alice = create_vauchi_with_identity("Alice");
    alice
        .storage()
        .recovery()
        .save_recovery_settings(&RecoverySettings::new(1, 1).unwrap())
        .unwrap();
    let alice_pk = signing_pk(&alice);
    let mut guardians: Vec<Recipient> = (0..3)
        .map(|index| add_recipient(&alice, &alice_pk, &format!("G{index}")))
        .collect();
    for guardian in &guardians {
        alice
            .verify_contact_fingerprint(&guardian.id_at_sharer)
            .unwrap();
        alice.toggle_recovery_trust(&guardian.id_at_sharer).unwrap();
    }

    for guardian in &mut guardians {
        receive(&alice, guardian);
        let g = guardian.vauchi();
        let kept: GuardianDelivery = g
            .storage()
            .guardian_backups()
            .kept(guardian.sharer_id())
            .unwrap()
            .expect("a kept backup");
        let own_share = g
            .respond_to_recovery(kept.sealed_share(), &signing_pk(g))
            .unwrap();

        let opened = g.recover_guardian_backup(&hex::encode(kept.backup()), &[own_share]);

        assert!(opened.is_err(), "one guardian's share opened the backup");
    }
}
