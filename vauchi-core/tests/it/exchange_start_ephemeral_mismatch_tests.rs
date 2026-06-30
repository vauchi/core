// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Diagnosis: the CLI's independent `start` + `complete` exchange flow derives
//! MISMATCHED shared secrets, so the two sides compute different daily mailbox
//! tokens and a relay sync is "sent, not received".
//!
//! `cli/src/commands/exchange.rs::start` generates a QR (an ephemeral) and
//! RETURNS without persisting the session — the ephemeral is discarded. A later
//! `complete` on the same identity runs its OWN `StartQR` (a fresh ephemeral)
//! and processes the PEER's *start* QR. The X3DH secret combines BOTH parties'
//! ephemerals, so each side ends up keyed off a different ephemeral pair:
//! Alice = (E_A', E_B), Bob = (E_B', E_A) — no shared ephemeral.
//!
//! Contrast `exchanged_ratchet_roundtrip_tests`, where the SAME two sessions
//! exchange their live QRs and the secrets match. The mailbox token is
//! `compute_mailbox_token(shared_key, recipient_pubkey, day)`
//! (network/mailbox_token.rs); both the poster and the poller key it to the
//! SAME recipient (Bob), so the shared secret is the only differing input —
//! mismatched secrets ⇒ mismatched tokens ⇒ the initiator posts to a token
//! the responder never polls.
//!
//! NB: this is a CLI-flow defect (separate process invocations, no persisted
//! session). The mobile/desktop live handshake exchanges current-session QRs,
//! so it does NOT hit this.
//!
//! Problem record: 2026-06-28-sync-delivery-sent-not-received (task #14).
//! Feature: features/contact_exchange.feature

use crate::common;

use common::helpers::create_vauchi_with_card;
use vauchi_core::clock::SystemClock;
use vauchi_core::exchange::{ExchangeEvent, ExchangeSession, MockProximityVerifier};
use vauchi_core::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};
use vauchi_core::{ContactCard, FieldType, Identity, Vauchi};

fn owned_identity_copy(wb: &Vauchi) -> Identity {
    let id = wb.identity().expect("identity present");
    let password = "Str0ng-Test-Passw0rd!";
    let backup = id.export_backup(password).expect("export backup");
    Identity::import_backup(&backup, password, 0).expect("import backup")
}

fn new_session(identity: Identity, card: ContactCard) -> ExchangeSession {
    ExchangeSession::new_qr(
        identity,
        card,
        MockProximityVerifier::success(),
        SystemClock::shared(),
    )
}

/// Drive a `complete`-style session to `Complete` against a peer's *start* QR
/// (the session runs its own `StartQR` first, as `cli complete` does), and
/// return the resulting contact (which carries the derived shared secret).
fn complete_against(
    identity: Identity,
    card: ContactCard,
    peer_start_qr: vauchi_core::exchange::ExchangeQR,
    peer_card: ContactCard,
) -> vauchi_core::Contact {
    let mut s = new_session(identity, card);
    s.apply(ExchangeEvent::StartQR).unwrap();
    s.apply(ExchangeEvent::ProcessQR(peer_start_qr)).unwrap();
    s.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    s.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    s.apply(ExchangeEvent::CompleteExchange(peer_card)).unwrap();
    s.extract_contact().expect("reached Complete")
}

// @scenario: contact_exchange :: Independent start+complete derives mismatched secrets
#[test]
fn cli_independent_start_complete_mismatches_shared_secret_and_mailbox_token() {
    let alice = create_vauchi_with_card("Alice", vec![(FieldType::Email, "work", "alice@old.com")]);
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "bob@old.com")]);
    let alice_card = alice.own_card().unwrap().unwrap();
    let bob_card = bob.own_card().unwrap().unwrap();

    // `exchange start`: each side generates a QR (an ephemeral) and discards
    // the session.
    let mut alice_start = new_session(owned_identity_copy(&alice), alice_card.clone());
    alice_start.apply(ExchangeEvent::StartQR).unwrap();
    let alice_start_qr = alice_start.qr().unwrap().clone();

    let mut bob_start = new_session(owned_identity_copy(&bob), bob_card.clone());
    bob_start.apply(ExchangeEvent::StartQR).unwrap();
    let bob_start_qr = bob_start.qr().unwrap().clone();
    drop(alice_start);
    drop(bob_start);

    // `exchange complete <peer-start-data>`: a FRESH session (new ephemeral)
    // processes the peer's start QR.
    let bob_at_alice = complete_against(
        owned_identity_copy(&alice),
        alice_card.clone(),
        bob_start_qr,
        bob_card.clone(),
    );
    let alice_at_bob = complete_against(
        owned_identity_copy(&bob),
        bob_card,
        alice_start_qr,
        alice_card,
    );

    // The two sides derive DIFFERENT shared secrets — the defect.
    let sa = *bob_at_alice
        .shared_key()
        .expect("alice's secret")
        .as_bytes();
    let sb = *alice_at_bob.shared_key().expect("bob's secret").as_bytes();
    assert_ne!(
        sa, sb,
        "independent start+complete must (defectively) derive different secrets"
    );

    // Therefore their daily mailbox tokens differ — the initiator posts to a
    // token the responder never polls ⇒ sent-not-received.
    // Directional tokens (ADR-029): Alice posts to Bob's mailbox and Bob polls
    // his own, so both derivations key to Bob's identity pubkey — leaving the
    // mismatched shared secret as the sole splitting input.
    let bob_pubkey = bob_at_alice.public_key().expect("bob's signing pubkey");
    let day = current_day_epoch(SystemClock::shared().unix_seconds());
    assert_ne!(
        token_hex(&compute_mailbox_token(&sa, bob_pubkey, day)),
        token_hex(&compute_mailbox_token(&sb, bob_pubkey, day)),
        "mismatched secrets must yield mismatched mailbox tokens"
    );
}
