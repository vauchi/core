// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A peer's CONF can land before this side reaches Confirming: the peer
//! shows it while we are still processing its reveal key. On a Pixel 3a ↔
//! iPhone SE Hover run the Pixel decoded the iPhone's CONF fifteen times in
//! Verifying, reached Confirming 50 ms after the last one, and the iPhone
//! had already moved on (issue #315, exchange journal D12).

use sha2::{Digest, Sha256};
use vauchi_core::exchange::multistage::qr_codec;
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ProtocolState;

struct Staged {
    bob: MultiStageSession,
    alice_session_id: [u8; 16],
    alice_verify: String,
}

/// Drives both sessions until Bob reaches Verifying, then lets Alice run on
/// (reading Bob) until she has shown her VRFY — which Bob never sees here.
fn bob_verifying_with_alice_verify_withheld(alice_card: &[u8], bob_card: &[u8]) -> Staged {
    let mut alice = MultiStageSession::new(alice_card.to_vec());
    let mut bob = MultiStageSession::new(bob_card.to_vec());

    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    for _ in 0..500 {
        if bob.get_state() == ProtocolState::Verifying {
            break;
        }
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq
            && !aq.data.starts_with("VRFY")
            && !aq.data.starts_with("CONF")
            && !aq.data.starts_with("CMBO")
        {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
    }
    assert_eq!(bob.get_state(), ProtocolState::Verifying);

    let mut alice_verify = None;
    for _ in 0..500 {
        if let Some(aq) = alice.get_display_qr()
            && aq.data.starts_with("VRFY")
        {
            alice_verify = Some(aq.data);
            break;
        }
        if let Some(bq) = bob.get_display_qr() {
            alice.process_scanned_qr(&bq.data);
        }
    }
    assert_eq!(
        bob.get_state(),
        ProtocolState::Verifying,
        "Bob must still be waiting for Alice's reveal key"
    );

    Staged {
        alice_session_id: alice.session_id(),
        alice_verify: alice_verify.expect("Alice never showed a VRFY"),
        bob,
    }
}

// @internal
#[test]
fn a_confirm_decoded_before_confirming_still_completes_the_exchange() {
    let alice_card = b"Alice (iPhone)".to_vec();
    let Staged {
        mut bob,
        alice_session_id,
        alice_verify,
    } = bob_verifying_with_alice_verify_withheld(&alice_card, b"Bob (Pixel)");
    let alice_card_hash: [u8; 32] = Sha256::digest(&alice_card).into();

    bob.process_scanned_qr(&qr_codec::format_confirm_qr(
        &alice_session_id,
        &alice_card_hash,
    ));
    bob.process_scanned_qr(&alice_verify);

    assert_eq!(
        bob.get_state(),
        ProtocolState::Complete,
        "the CONF Bob already decoded must count once he reaches Confirming"
    );
}

// @internal
#[test]
fn an_early_confirm_with_the_wrong_hash_still_fails_closed() {
    let Staged {
        mut bob,
        alice_session_id,
        alice_verify,
    } = bob_verifying_with_alice_verify_withheld(b"Alice (iPhone)", b"Bob (Pixel)");
    let forged_hash: [u8; 32] = Sha256::digest(b"not Alice's card").into();

    bob.process_scanned_qr(&qr_codec::format_confirm_qr(
        &alice_session_id,
        &forged_hash,
    ));
    bob.process_scanned_qr(&alice_verify);

    assert!(
        matches!(bob.get_state(), ProtocolState::Failed(_)),
        "a kept CONF must be checked like a fresh one, got {:?}",
        bob.get_state()
    );
}
