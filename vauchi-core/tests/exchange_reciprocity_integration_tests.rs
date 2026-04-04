// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests proving two exchange sessions produce cross-matching
//! confirmation tokens (HKDF derivation symmetry).

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::{ExchangeEvent, ExchangeSession, MockProximityVerifier};
use vauchi_core::identity::Identity;

/// Drives two sessions through a full mutual QR exchange up to key agreement.
/// Returns (session_a, session_b).
fn drive_mutual_qr_exchange() -> (ExchangeSession, ExchangeSession) {
    let identity_a = Identity::create("Alice");
    let identity_b = Identity::create("Bob");

    let mut session_a = ExchangeSession::new_qr(
        identity_a,
        ContactCard::new("Alice"),
        MockProximityVerifier::success(),
    );
    let mut session_b = ExchangeSession::new_qr(
        identity_b,
        ContactCard::new("Bob"),
        MockProximityVerifier::success(),
    );

    // Both start QR display
    session_a.apply(ExchangeEvent::StartQR).unwrap();
    session_b.apply(ExchangeEvent::StartQR).unwrap();

    // A scans B's QR, B scans A's QR (mutual exchange)
    let qr_a = session_a.qr().unwrap().clone();
    let qr_b = session_b.qr().unwrap().clone();

    session_a.apply(ExchangeEvent::ProcessQR(qr_b)).unwrap();
    session_b.apply(ExchangeEvent::ProcessQR(qr_a)).unwrap();

    session_a.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    session_b.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    // Both perform key agreement
    session_a.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    session_b.apply(ExchangeEvent::PerformKeyAgreement).unwrap();

    (session_a, session_b)
}

// @internal
#[test]
fn tokens_cross_match_between_sessions() {
    let (session_a, session_b) = drive_mutual_qr_exchange();

    let a_our = session_a.our_confirmation_token().unwrap();
    let a_their = session_a.expected_their_token().unwrap();
    let b_our = session_b.our_confirmation_token().unwrap();
    let b_their = session_b.expected_their_token().unwrap();

    // Cross-matching: A's our_token == B's expected_their_token
    assert_eq!(a_our, b_their, "A's token must match what B expects");
    assert_eq!(b_our, a_their, "B's token must match what A expects");

    // Self-asymmetry: each side's own tokens are different
    assert_ne!(a_our, a_their, "A's tokens must differ (echo protection)");
    assert_ne!(b_our, b_their, "B's tokens must differ (echo protection)");
}

// @internal
#[test]
fn escrow_keys_share_gate_but_swap_slots() {
    let (session_a, session_b) = drive_mutual_qr_exchange();

    let (a_gate, a_our_slot, a_their_slot) = session_a.confirmation_escrow().unwrap();
    let (b_gate, b_our_slot, b_their_slot) = session_b.confirmation_escrow().unwrap();

    // Same gate (both derive from same shared secret)
    assert_eq!(a_gate, b_gate, "both sides must derive same escrow gate");

    // Swapped slots (role-dependent)
    assert_eq!(
        a_our_slot, b_their_slot,
        "A's our_slot must be B's their_slot"
    );
    assert_eq!(
        a_their_slot, b_our_slot,
        "A's their_slot must be B's our_slot"
    );
}

// @internal
#[test]
fn different_sessions_produce_different_tokens() {
    let (session_a1, _) = drive_mutual_qr_exchange();
    let (session_a2, _) = drive_mutual_qr_exchange();

    // Different sessions (different ephemeral keys) produce different tokens
    assert_ne!(
        session_a1.our_confirmation_token().unwrap(),
        session_a2.our_confirmation_token().unwrap(),
        "different exchange sessions must produce different tokens"
    );
}
