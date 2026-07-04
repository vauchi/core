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
///
/// Ends with the production ratchet bootstrap: the Double-Ratchet initiator
/// (decided by identity-key comparison — stable per pair, not choosable)
/// sends its initial card and the responder decrypts it, which births the
/// responder's sending chain. After that, `… syncs their card with …` works
/// in either direction, so scenarios may name either party as the updater.
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

    let b_at_a = a_s.extract_contact().expect("first party reached Complete");
    let a_at_b = b_s
        .extract_contact()
        .expect("second party reached Complete");
    let (a_ratchet, a_init) = a_s.build_exchange_ratchet(&b_at_a).unwrap();
    let (b_ratchet, b_init) = b_s.build_exchange_ratchet(&a_at_b).unwrap();

    a.save_exchanged_contact(&b_at_a, &a_ratchet, a_init)
        .unwrap();
    b.save_exchanged_contact(&a_at_b, &b_ratchet, b_init)
        .unwrap();

    // Initiator's initial-card send. The empty-display-name baseline makes
    // the delta non-empty even for a fieldless card, and the responder's
    // decrypt is what establishes its sending chain (DR bootstrap).
    let (init, resp, resp_id_at_init, init_id_at_resp) = if a_init {
        (a, b, b_at_a.id().to_string(), a_at_b.id().to_string())
    } else {
        (b, a, a_at_b.id().to_string(), b_at_a.id().to_string())
    };
    let baseline = vauchi_core::ContactCard::new("");
    let current = init.own_card().unwrap().unwrap();
    let ciphertext = init
        .prepare_card_update_for_contact(&resp_id_at_init, &baseline, &current)
        .unwrap();
    vauchi_core::api::process_single_card_update(
        resp.identity().unwrap(),
        resp.storage(),
        &init_id_at_resp,
        &ciphertext,
    )
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

/// Resolves the contact id `peer` has in `owner`'s contact list, by the
/// display name established at exchange time.
fn contact_id_at(owner: &Vauchi, peer: &str) -> String {
    owner
        .list_contacts()
        .unwrap()
        .iter()
        .find(|c| c.display_name() == peer)
        .unwrap_or_else(|| panic!("no contact named {peer:?} in owner's list"))
        .id()
        .to_string()
}

#[given(expr = "{string} has a/an {word} field {string} with value {string}")]
fn party_field(world: &mut VauchiWorld, party: String, kind: String, label: String, value: String) {
    use vauchi_core::{ContactField, FieldType};
    let field_type = match kind.as_str() {
        "phone" => FieldType::Phone,
        "email" => FieldType::Email,
        "address" => FieldType::Address,
        other => panic!("unsupported field kind {other:?}"),
    };
    world
        .party(&party)
        .add_own_field(ContactField::new(field_type, &label, &value, 0))
        .unwrap();
}

#[when(expr = "{string} updates their {string} field to {string}")]
fn party_updates_field(world: &mut VauchiWorld, party: String, label: String, value: String) {
    let v = world.party(&party);
    let old = v.own_card().unwrap().unwrap();
    let field_id = old
        .fields()
        .iter()
        .find(|f| f.label() == label)
        .unwrap_or_else(|| panic!("{party} has no field labeled {label:?}"))
        .id()
        .to_string();
    let mut new = old.clone();
    new.update_field_value(&field_id, &value, 1).unwrap();
    v.update_own_card(&new).unwrap();
    world.old_cards.insert(party, old);
}

/// Shuttles the pending card update sender→recipient through the real seams
/// (`prepare_card_update_for_contact` → `process_single_card_update`) — the
/// same CEK-wrapped ciphertext a relay would carry, minus the transport.
#[when(expr = "{string} syncs their card with {string}")]
fn party_syncs_with(world: &mut VauchiWorld, sender: String, recipient: String) {
    let old = world
        .old_cards
        .remove(&sender)
        .unwrap_or_else(|| panic!("{sender} has no pending card update to sync"));
    let s = world.party(&sender);
    let r = world.party(&recipient);
    let new = s.own_card().unwrap().unwrap();
    let recipient_id = contact_id_at(s, &recipient);
    let ciphertext = s
        .prepare_card_update_for_contact(&recipient_id, &old, &new)
        .unwrap();
    let sender_id = contact_id_at(r, &sender);
    vauchi_core::api::process_single_card_update(
        r.identity().unwrap(),
        r.storage(),
        &sender_id,
        &ciphertext,
    )
    .unwrap();
}

#[then(expr = "{string} sees {string} field {string} with value {string}")]
fn party_sees_peer_field(
    world: &mut VauchiWorld,
    viewer: String,
    peer: String,
    label: String,
    value: String,
) {
    let v = world.party(&viewer);
    let peer_id = contact_id_at(v, &peer);
    let contact = v.get_contact(&peer_id).unwrap().unwrap();
    let got = contact
        .card()
        .fields()
        .iter()
        .find(|f| f.label() == label)
        .unwrap_or_else(|| panic!("{viewer} sees no {label:?} field on {peer}'s card"))
        .value()
        .to_string();
    assert_eq!(got, value, "{viewer}'s view of {peer}'s {label} field");
}
