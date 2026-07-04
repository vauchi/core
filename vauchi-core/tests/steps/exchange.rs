// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-party (two-World) step vocabulary: two named `Vauchi` instances drive
//! a real X3DH mutual QR exchange, ending with each holding the other as a
//! contact + a working Double Ratchet. This opens the exchange/sync features
//! (the largest, previously 100%-unbound block) to the cucumber suite.
//!
//! The exchange runs core's pure-I/O `ExchangeSession` (no hardware,
//! `MockProximityVerifier`), mirroring `exchange_real_x3dh_card_update_tests`.

use cucumber::{given, then, when};
use vauchi_core::clock::SystemClock;
use vauchi_core::exchange::{ExchangeEvent, ExchangeSession, MockProximityVerifier};
use vauchi_core::{Identity, Vauchi};

use crate::VauchiWorld;

/// An `ExchangeSession` consumes an owned `Identity`; round-trip a backup to
/// get one without disturbing the party's live identity.
fn owned_identity_copy(v: &Vauchi) -> Identity {
    let id = v.identity().expect("identity present");
    let password = "Str0ng-Test-Passw0rd!";
    let backup = id.export_backup(password).expect("export backup");
    Identity::import_backup(&backup, password, 0).expect("import backup")
}

/// Drive two parties through a full mutual QR exchange and persist the result
/// on both sides via the real `save_exchanged_contact` seam.
fn qr_exchange(a: &Vauchi, b: &Vauchi) {
    let a_card = a.own_card().unwrap().unwrap();
    let b_card = b.own_card().unwrap().unwrap();
    let mut a_s = ExchangeSession::new_qr(
        owned_identity_copy(a),
        a_card.clone(),
        MockProximityVerifier::success(),
        SystemClock::shared(),
    );
    let mut b_s = ExchangeSession::new_qr(
        owned_identity_copy(b),
        b_card.clone(),
        MockProximityVerifier::success(),
        SystemClock::shared(),
    );

    a_s.apply(ExchangeEvent::StartQR).unwrap();
    b_s.apply(ExchangeEvent::StartQR).unwrap();
    let a_qr = a_s.qr().unwrap().clone();
    let b_qr = b_s.qr().unwrap().clone();
    a_s.apply(ExchangeEvent::ProcessQR(b_qr)).unwrap();
    b_s.apply(ExchangeEvent::ProcessQR(a_qr)).unwrap();
    a_s.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    b_s.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    a_s.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    b_s.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    a_s.apply(ExchangeEvent::CompleteExchange(b_card)).unwrap();
    b_s.apply(ExchangeEvent::CompleteExchange(a_card)).unwrap();

    let b_at_a = a_s.extract_contact().expect("alice reached Complete");
    let a_at_b = b_s.extract_contact().expect("bob reached Complete");
    let (a_ratchet, a_init) = a_s.build_exchange_ratchet(&b_at_a).unwrap();
    let (b_ratchet, b_init) = b_s.build_exchange_ratchet(&a_at_b).unwrap();

    a.save_exchanged_contact(&b_at_a, &a_ratchet, a_init)
        .unwrap();
    b.save_exchanged_contact(&a_at_b, &b_ratchet, b_init)
        .unwrap();
}

#[given(expr = "a user {string}")]
fn a_user(world: &mut VauchiWorld, name: String) {
    let mut v = Vauchi::in_memory().unwrap();
    v.create_identity(&name).unwrap();
    world.parties.insert(name, v);
}

#[when(expr = "{string} and {string} complete a QR exchange")]
fn complete_qr_exchange(world: &mut VauchiWorld, a: String, b: String) {
    qr_exchange(world.party(&a), world.party(&b));
}

#[then(expr = "{string} has {string} as a contact")]
fn has_contact(world: &mut VauchiWorld, owner: String, peer: String) {
    let contacts = world.party(&owner).list_contacts().unwrap();
    assert!(
        contacts.iter().any(|c| c.display_name() == peer),
        "expected {owner} to have {peer} as a contact"
    );
}
