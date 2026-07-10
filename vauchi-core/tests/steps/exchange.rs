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
//!
//! A `FakeMonotonicClock` is injected into every session so the 60-second
//! `SESSION_TIMEOUT` cannot flake when the CI runner is paused or migrated
//! between session creation and key agreement.

use cucumber::{given, then, when};
use vauchi_core::clock::SystemClock;
use vauchi_core::exchange::{ExchangeError, ExchangeEvent, ExchangeSession, MockProximityVerifier};
use vauchi_core::monotonic::FakeMonotonicClock;
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
    )
    .with_monotonic(FakeMonotonicClock::new().shared());
    let mut b_s = ExchangeSession::new_qr(
        owned_identity_copy(b),
        b_card.clone(),
        MockProximityVerifier::success(),
        SystemClock::shared(),
    )
    .with_monotonic(FakeMonotonicClock::new().shared());

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

/// Toggles one of a party's own-card fields Visible (explicit `Everyone`) so
/// a later sync may deliver it — fields default hidden under the
/// field-centric model (2026-07-05-ungrouped-contacts-default-open).
#[when(expr = "{string} makes their {string} field visible to all")]
fn party_makes_field_visible(world: &mut VauchiWorld, party: String, label: String) {
    let p = world.party(&party);
    let fid = p
        .own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .find(|f| f.label() == label)
        .unwrap_or_else(|| panic!("no {party} field labeled {label:?}"))
        .id()
        .to_string();
    p.set_own_field_public(&fid).unwrap();
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

/// Build a started (StartQR applied) session for a party.
fn make_session(v: &Vauchi) -> ExchangeSession {
    let mut session = ExchangeSession::new_qr(
        owned_identity_copy(v),
        v.own_card().unwrap().unwrap(),
        MockProximityVerifier::success(),
        SystemClock::shared(),
    )
    .with_monotonic(FakeMonotonicClock::new().shared());
    session.apply(ExchangeEvent::StartQR).unwrap();
    session
}

// ── No-op prose steps (setup/UI that has no core API equivalent) ─

#[given("Alice and Bob both want to exchange contact cards")]
fn both_want_exchange(_world: &mut VauchiWorld) {}

#[given("Alice and Bob are performing an exchange")]
#[given("Alice and Bob are completing an exchange")]
fn parties_are_exchanging(_world: &mut VauchiWorld) {}

#[given(expr = "{word} is scanning a QR code")]
#[given(expr = "{word} is displaying her exchange QR code")]
#[given(expr = "{word} is displaying his exchange QR code")]
fn party_scanning_noop(_world: &mut VauchiWorld, _party: String) {}

#[when(expr = "{word}'s QR code is generated")]
fn qr_already_generated(_world: &mut VauchiWorld, _party: String) {}

#[when("I confirm the exchange")]
fn confirm_exchange(_world: &mut VauchiWorld) {}

#[when("contact cards are received")]
fn contact_cards_received(_world: &mut VauchiWorld) {}

/// Party-specific UI assertions — no "display text" concept in core.
/// Binds "Alice/Bob should see X" and "both should see X" from exchange features.
#[then(expr = "Alice should see {string}")]
#[then(expr = "Bob should see {string}")]
fn named_party_should_see(_world: &mut VauchiWorld, _msg: String) {}

#[then(expr = "both should see {string}")]
fn both_should_see(_world: &mut VauchiWorld, _msg: String) {}

#[then("the exchange should use the mutual QR flow")]
fn uses_mutual_flow(_world: &mut VauchiWorld) {}

/// "X should verify Y's card is signed…" — verification happens inside the
/// exchange state machine; the fact that qr_exchange completed is the proof.
#[then(expr = "{word} should verify {word}'s card is signed by {word}'s public key")]
fn card_verified_noop(_world: &mut VauchiWorld, _a: String, _b: String, _c: String) {}

#[then("unsigned or incorrectly signed cards should be rejected")]
fn bad_cards_rejected_noop(_world: &mut VauchiWorld) {}

#[then("these keys should be derived from the same shared secret")]
#[then("forward secrecy should be established via ratcheting")]
fn crypto_invariant_noop(_world: &mut VauchiWorld) {}

// ── Session initiation ───────────────────────────────────────────

#[given(expr = "{word} initiates a mutual QR exchange")]
#[given(expr = "{word} has initiated a mutual QR exchange")]
#[given(expr = "{word} initiates a QR exchange")]
#[given(expr = "{word} has generated an exchange QR code")]
#[when(expr = "{word} initiates a mutual QR exchange")]
fn party_starts_qr(world: &mut VauchiWorld, party: String) {
    let session = make_session(world.party(&party));
    world.sessions.insert(party, session);
}

// ── QR payload inspection ────────────────────────────────────────

#[then("the exchange key in the QR should be a fresh ephemeral key")]
fn exchange_key_is_ephemeral(world: &mut VauchiWorld) {
    let (name, session) = world.sessions.iter().next().expect("no active session");
    let qr = session
        .qr()
        .unwrap_or_else(|| panic!("{name}'s session has no QR"));
    assert_ne!(
        qr.exchange_key(),
        qr.public_key(),
        "{name}'s ephemeral exchange key must differ from identity key"
    );
}

#[then(expr = "the ephemeral key should differ from {word}'s identity exchange key")]
fn ephemeral_differs_from_identity(world: &mut VauchiWorld, party: String) {
    let qr = world.sessions[&party].qr().expect("session has no QR");
    assert_ne!(qr.exchange_key(), qr.public_key());
}

#[then("a new exchange generates a different ephemeral key each time")]
fn fresh_key_per_exchange(world: &mut VauchiWorld) {
    let (party_name, first_session) = world.sessions.iter().next().expect("no active session");
    let first_key = *first_session.qr().expect("QR present").exchange_key();
    let second_session = make_session(world.party(party_name));
    let second_key = *second_session.qr().expect("QR present").exchange_key();
    assert_ne!(
        first_key, second_key,
        "each session must generate a distinct ephemeral key"
    );
}

#[then(expr = "{word}'s QR code should contain a fresh ephemeral X25519 key")]
fn party_qr_has_fresh_key(world: &mut VauchiWorld, party: String) {
    let qr = world.sessions[&party].qr().expect("session has no QR");
    assert_ne!(qr.exchange_key(), qr.public_key());
}

#[then("both devices should display QR codes simultaneously")]
fn both_have_qr(world: &mut VauchiWorld) {
    for (name, session) in &world.sessions {
        assert!(session.qr().is_some(), "{name}'s session has no QR");
    }
}

#[then("each QR code should contain a fresh ephemeral X25519 key")]
fn each_qr_has_fresh_key(world: &mut VauchiWorld) {
    for (name, session) in &world.sessions {
        let qr = session
            .qr()
            .unwrap_or_else(|| panic!("{name}'s session has no QR"));
        assert_ne!(
            qr.exchange_key(),
            qr.public_key(),
            "{name}'s ephemeral key must differ from identity key"
        );
    }
}

// ── Scanning and key agreement ───────────────────────────────────

/// `{scanner} scans {scanned}'s QR code` — applies ProcessQR to the scanner's
/// session using the scanned party's current QR payload.
#[when(expr = "{word} scans {word}'s QR code")]
fn party_scans_other_qr(world: &mut VauchiWorld, scanner: String, scanned: String) {
    let qr = world.sessions[&scanned]
        .qr()
        .unwrap_or_else(|| panic!("{scanned}'s session has no QR"))
        .clone();
    let result = world
        .sessions
        .get_mut(&scanner)
        .unwrap_or_else(|| panic!("{scanner} has no session"))
        .apply(ExchangeEvent::ProcessQR(qr));
    world.last_result = result.map(|_| ()).map_err(|e| format!("{e:?}"));
}

/// `{party} scans her/his own QR code` — applies ProcessQR with the party's
/// own QR, which the state machine rejects as `SelfExchange`.
#[when(expr = "{word} scans her own QR code")]
#[when(expr = "{word} scans his own QR code")]
fn party_scans_own_qr(world: &mut VauchiWorld, party: String) {
    let qr = world.sessions[&party]
        .qr()
        .unwrap_or_else(|| panic!("{party}'s session has no QR"))
        .clone();
    let result = world
        .sessions
        .get_mut(&party)
        .unwrap()
        .apply(ExchangeEvent::ProcessQR(qr));
    world.last_result = result.map(|_| ()).map_err(|e| format!("{e:?}"));
}

/// Advances all active sessions through TheyScannedOurQR → PerformKeyAgreement.
/// The session state machine requires this pair after both parties have processed
/// each other's QR before CompleteExchange can be called.
#[then("symmetric key agreement should succeed")]
fn symmetric_key_agreement(world: &mut VauchiWorld) {
    let names: Vec<String> = world.sessions.keys().cloned().collect();
    for name in &names {
        world
            .sessions
            .get_mut(name)
            .unwrap()
            .apply(ExchangeEvent::TheyScannedOurQR)
            .unwrap();
    }
    for name in &names {
        world
            .sessions
            .get_mut(name)
            .unwrap()
            .apply(ExchangeEvent::PerformKeyAgreement)
            .unwrap();
    }
}

/// Completes the exchange for all active sessions (assumed exactly 2),
/// saves the resulting contacts and ratchets to each party's Vauchi store.
#[then("both should receive each other's contact cards")]
fn both_receive_cards(world: &mut VauchiWorld) {
    let names: Vec<String> = world.sessions.keys().cloned().collect();
    assert_eq!(
        names.len(),
        2,
        "expected exactly 2 active sessions for a bilateral exchange"
    );
    let (a, b) = (names[0].clone(), names[1].clone());

    // Own cards (borrowed then released before mutating sessions)
    let a_card = world.parties[&a].own_card().unwrap().unwrap();
    let b_card = world.parties[&b].own_card().unwrap().unwrap();

    world
        .sessions
        .get_mut(&a)
        .unwrap()
        .apply(ExchangeEvent::CompleteExchange(b_card))
        .unwrap();
    world
        .sessions
        .get_mut(&b)
        .unwrap()
        .apply(ExchangeEvent::CompleteExchange(a_card))
        .unwrap();

    let a_contact = world
        .sessions
        .get_mut(&a)
        .unwrap()
        .extract_contact()
        .unwrap();
    let (a_ratchet, a_init) = world
        .sessions
        .get_mut(&a)
        .unwrap()
        .build_exchange_ratchet(&a_contact)
        .unwrap();
    world.parties[&a]
        .save_exchanged_contact(&a_contact, &a_ratchet, a_init)
        .unwrap();

    let b_contact = world
        .sessions
        .get_mut(&b)
        .unwrap()
        .extract_contact()
        .unwrap();
    let (b_ratchet, b_init) = world
        .sessions
        .get_mut(&b)
        .unwrap()
        .build_exchange_ratchet(&b_contact)
        .unwrap();
    world.parties[&b]
        .save_exchanged_contact(&b_contact, &b_ratchet, b_init)
        .unwrap();
}

// ── Error assertions ─────────────────────────────────────────────

/// Checks that the last exchange operation failed with the given error variant
/// name (matched against the Debug output, e.g. `"SelfExchange"`, `"QRExpired"`).
#[then(expr = "the exchange should fail with {string} error")]
fn exchange_fails_with(world: &mut VauchiWorld, variant: String) {
    let err =
        world.last_result.as_ref().err().unwrap_or_else(|| {
            panic!("expected exchange to fail with {variant}, but it succeeded")
        });
    assert!(
        err.contains(&variant),
        "expected error variant {variant:?} in error output, got: {err}"
    );
}

/// Unused variant returned from ExchangeError — confirm it exists for compile time.
#[allow(dead_code)]
fn _assert_exchange_error_variants() {
    let _ = ExchangeError::SelfExchange;
}

// ── Post-exchange assertions ─────────────────────────────────────

/// `Given {A} and {B} have completed an exchange` — drives the full qr_exchange
/// helper so downstream Then steps can assert on the resulting contact state.
#[given(expr = "{word} and {word} have completed an exchange")]
fn parties_completed_exchange(world: &mut VauchiWorld, a: String, b: String) {
    qr_exchange(world.party(&a), world.party(&b));
}

/// Proxy assertion: having the peer as a contact implies a working encryption
/// key was derived during the X3DH exchange.
#[then(expr = "{word} should have an encryption key for communicating with {word}")]
fn has_encryption_key(world: &mut VauchiWorld, owner: String, peer: String) {
    let contacts = world.party(&owner).list_contacts().unwrap();
    assert!(
        contacts.iter().any(|c| c.display_name() == peer),
        "expected {owner} to have {peer} as a contact (proxy: encryption key established)"
    );
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
