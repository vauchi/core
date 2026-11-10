// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! In the recovery meeting a guardian vouches, exchanges with the new
//! identity, and hands back its share re-sealed to the new key with the
//! backup (vauchi/private#796 step 2b, D13). It hands back only to the new
//! key of a claim it vouched for, while that claim may still be valid;
//! anyone else is refused, and a duress session holds nothing to return.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_core::api::process_single_card_update;
use vauchi_core::clock::FakeClock;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::recovery::RecoveryClaim;
use vauchi_core::sync::guardian_piece::GuardianDelivery;
use vauchi_core::{AuthMode, SymmetricKey, Vauchi, VauchiError};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::two_recipient::{Recipient, add_recipient, add_recipient_from, receive};

fn signing_pk(wb: &Vauchi) -> [u8; 32] {
    *wb.identity().unwrap().signing_public_key()
}

/// Makes `a` and `b` exchanged contacts with a ratchet, `a` the initiator;
/// returns `b`'s id at `a` and `a`'s id at `b`.
fn exchange(a: &Vauchi, b: &Vauchi) -> (String, String) {
    let shared = SymmetricKey::generate();
    let b_at_a = Contact::from_exchange(signing_pk(b), ContactCard::new("B"), shared.clone(), 0);
    let a_at_b = Contact::from_exchange(signing_pk(a), ContactCard::new("A"), shared.clone(), 0);
    let (b_id, a_id) = (b_at_a.id().to_string(), a_at_b.id().to_string());
    a.add_contact(b_at_a).unwrap();
    b.add_contact(a_at_b).unwrap();
    let b_dh = X3DHKeyPair::generate();
    a.create_ratchet_as_initiator(&b_id, &shared, *b_dh.public_key())
        .unwrap();
    b.create_ratchet_as_responder(&a_id, &shared, b_dh).unwrap();
    (b_id, a_id)
}

/// Delivers everything `from` queued for `to_id` into `to`, as from `from_id`.
fn deliver(from: &Vauchi, to_id: &str, to: &Vauchi, from_id: &str) {
    for update in from.storage().pending().get_pending_updates(to_id).unwrap() {
        process_single_card_update(
            to.identity().unwrap(),
            to.storage(),
            from_id,
            &update.payload,
        )
        .unwrap();
    }
}

/// Alice with three guardians that keep her backup.
fn alice_and_guardians() -> (Vauchi, Vec<Recipient>) {
    let alice = create_vauchi_with_identity("Alice");
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
    }
    (alice, guardians)
}

// @internal
#[test]
fn three_guardians_hand_back_in_the_meeting_and_the_new_phone_opens_the_backup() {
    let (alice, guardians) = alice_and_guardians();
    let new_phone = create_vauchi_with_identity("Alice, new phone");
    let claim = new_phone
        .create_recovery_claim_hex_b64(&hex::encode(signing_pk(&alice)))
        .unwrap();

    let mut shares = Vec::new();
    let mut backup = None;
    for guardian in &guardians {
        let g = guardian.vauchi();
        g.create_voucher_from_claim_b64(&claim).unwrap();
        let (phone_at_g, g_at_phone) = exchange(g, &new_phone);

        g.return_guardian_backup(&phone_at_g).unwrap();

        deliver(g, &phone_at_g, &new_phone, &g_at_phone);
        let returned: GuardianDelivery = new_phone
            .storage()
            .returned_backups()
            .kept(&g_at_phone)
            .unwrap()
            .expect("a returned backup");
        shares.push(returned.sealed_share().to_vec());
        backup = Some(returned.backup().to_vec());
    }

    let envelope = new_phone
        .recover_guardian_backup(&hex::encode(backup.unwrap()), &shares)
        .unwrap();
    assert_eq!(envelope.sections.identity.display_name, "Alice");
}

/// The refusals: no vouch for this key, a vouch for another key, and the
/// old device's own key, which a vouched claim names as old, never as new.
// @internal
#[test]
fn a_guardian_hands_back_only_to_the_new_key_of_a_claim_it_vouched_for() {
    let (alice, guardians) = alice_and_guardians();
    let g = guardians[0].vauchi();
    let new_phone = create_vauchi_with_identity("Alice, new phone");
    let stranger = create_vauchi_with_identity("Stranger");
    let (stranger_at_g, _) = exchange(g, &stranger);

    assert!(matches!(
        g.return_guardian_backup(&stranger_at_g),
        Err(VauchiError::InvalidState(_))
    ));

    let claim = new_phone
        .create_recovery_claim_hex_b64(&hex::encode(signing_pk(&alice)))
        .unwrap();
    g.create_voucher_from_claim_b64(&claim).unwrap();

    assert!(matches!(
        g.return_guardian_backup(&stranger_at_g),
        Err(VauchiError::InvalidState(_))
    ));
    assert!(matches!(
        g.return_guardian_backup(guardians[0].sharer_id()),
        Err(VauchiError::InvalidState(_))
    ));
    assert_eq!(
        g.storage()
            .pending()
            .get_pending_updates(&stranger_at_g)
            .unwrap()
            .len(),
        0
    );
}

/// A vouch lets the guardian hand back for as long as the claim it
/// vouched for may live, and not a minute longer.
// @internal
#[test]
fn a_vouch_hands_back_while_its_claim_may_live_and_not_after() {
    let clock = Arc::new(FakeClock::new(SystemTime::now()));
    let mut clocked = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
    clocked.create_identity("G0").unwrap();
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = signing_pk(&alice);
    let mut guardians = vec![add_recipient_from(&alice, &alice_pk, clocked)];
    guardians.extend((1..3).map(|index| add_recipient(&alice, &alice_pk, &format!("G{index}"))));
    for guardian in &guardians {
        alice
            .verify_contact_fingerprint(&guardian.id_at_sharer)
            .unwrap();
        alice.toggle_recovery_trust(&guardian.id_at_sharer).unwrap();
    }
    for guardian in &mut guardians {
        receive(&alice, guardian);
    }
    let g = guardians[0].vauchi();
    let new_phone = create_vauchi_with_identity("Alice, new phone");
    let claim = new_phone
        .create_recovery_claim_hex_b64(&hex::encode(alice_pk))
        .unwrap();
    g.create_voucher_from_claim_b64(&claim).unwrap();
    let (phone_at_g, _) = exchange(g, &new_phone);

    clock.advance(Duration::from_secs(RecoveryClaim::MAX_AGE_SECS - 60));
    let within = g.return_guardian_backup(&phone_at_g);
    assert!(matches!(within, Ok(())), "got {within:?}");

    clock.advance(Duration::from_secs(120));
    assert!(matches!(
        g.return_guardian_backup(&phone_at_g),
        Err(VauchiError::InvalidState(_))
    ));
}

/// A coerced guardian answers as one that holds no backup: the same
/// refusal, nothing queued, so the duress session is not told apart.
// @internal
#[test]
fn a_duress_session_hands_back_nothing_and_answers_as_one_with_no_backup() {
    let (alice, mut guardians) = alice_and_guardians();
    let new_phone = create_vauchi_with_identity("Alice, new phone");
    let claim = new_phone
        .create_recovery_claim_hex_b64(&hex::encode(signing_pk(&alice)))
        .unwrap();
    let phone_at_g = {
        let g = guardians[0].vauchi();
        g.create_voucher_from_claim_b64(&claim).unwrap();
        exchange(g, &new_phone).0
    };
    let mut g = guardians.remove(0).into_vauchi();
    g.setup_app_password("normal-pin").unwrap();
    g.setup_duress_pin_with_biometric_unlock("112233").unwrap();
    assert_eq!(g.authenticate("112233").unwrap(), AuthMode::Duress);

    assert!(matches!(
        g.return_guardian_backup(&phone_at_g),
        Err(VauchiError::InvalidState(_))
    ));
    assert_eq!(
        g.storage()
            .pending()
            .get_pending_updates(&phone_at_g)
            .unwrap()
            .len(),
        0
    );
}
