// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The confirm: the reveal key and a tag keyed from the transport key that
//! confirms both payloads, session ids, ephemerals and commitments. One read
//! of it takes a peer that holds every chunk to Finalized, whatever
//! finalization state it is in (design D5); a phone shows it only once its
//! own checks of the peer passed (vauchi/private#1217).

use vauchi_core::exchange::multistage::confirmation::{self, Side, TagLabel};
use vauchi_core::exchange::multistage::qr_codec::{self, StageQr, parse_qr};
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::{AccelerometerProximityState, ProtocolState};

const ALICE_CARD: &[u8] = b"Alice (iPhone)";
const BOB_CARD: &[u8] = b"Bob (Pixel)";

struct Staged {
    alice: MultiStageSession,
    bob: MultiStageSession,
    alice_final: String,
    alice_init: String,
    bob_init: String,
}

fn is_confirm(frame: &str) -> bool {
    frame.starts_with("CNF4")
}

fn is_reveal_or_confirm(frame: &str) -> bool {
    frame.starts_with("RVL4") || is_confirm(frame)
}

/// Drives both sessions until Bob holds every chunk and waits in Verifying,
/// with Alice's confirm captured but never shown to him.
fn bob_verifying_with_alice_final_withheld() -> Staged {
    let mut alice = MultiStageSession::new(ALICE_CARD.to_vec());
    let mut bob = MultiStageSession::new(BOB_CARD.to_vec());

    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    let mut alice_final = None;
    for _ in 0..500 {
        // Bob reads neither Alice's reveal nor her confirm, so he holds her
        // chunks without the key to open them.
        if let Some(aq) = alice.get_display_qr() {
            if is_confirm(&aq.data) {
                alice_final = Some(aq.data);
            } else if !is_reveal_or_confirm(&aq.data) {
                bob.process_scanned_qr(&aq.data);
            }
        }
        // Alice reads Bob's reveal, so she can check him and confirm; his
        // confirm stays with him, so she cannot finish.
        if let Some(bq) = bob.get_display_qr()
            && !is_confirm(&bq.data)
        {
            alice.process_scanned_qr(&bq.data);
        }
        if bob.get_state() == ProtocolState::Verifying && alice_final.is_some() {
            break;
        }
    }
    assert_eq!(bob.get_state(), ProtocolState::Verifying);

    Staged {
        alice,
        bob,
        alice_final: alice_final.expect("Alice never showed a confirm"),
        alice_init: ai.data,
        bob_init: bi.data,
    }
}

fn init_of(frame: &str) -> ([u8; 16], [u8; 32], [u8; 32]) {
    let Ok(StageQr::Init {
        session_id,
        ephemeral,
        commitment_hash,
        ..
    }) = parse_qr(frame)
    else {
        panic!("not an opening frame");
    };
    (session_id, ephemeral, commitment_hash)
}

/// The tag Alice sends Bob, over the payloads and session ids given, keyed
/// from the session's transport key.
fn alice_tag(
    staged: &Staged,
    label: TagLabel,
    alice_payload: &[u8],
    bob_session_id: [u8; 16],
) -> [u8; 32] {
    let (a_sid, a_eph, a_ch) = init_of(&staged.alice_init);
    let (_, b_eph, b_ch) = init_of(&staged.bob_init);
    let transport_key = staged
        .bob
        .get_transport_key()
        .expect("the transport key exists once the opening frames are read");
    confirmation::tag(
        &transport_key,
        label,
        &Side {
            session_id: &a_sid,
            ephemeral: &a_eph,
            commitment_hash: &a_ch,
            payload: alice_payload,
        },
        &Side {
            session_id: &bob_session_id,
            ephemeral: &b_eph,
            commitment_hash: &b_ch,
            payload: BOB_CARD,
        },
    )
}

fn reveal_key_of(final_frame: &str) -> [u8; 32] {
    let Ok(StageQr::Final { reveal_key, .. }) = parse_qr(final_frame) else {
        panic!("not a final frame");
    };
    reveal_key
}

// @internal
#[test]
fn one_read_of_the_final_frame_finalizes_a_session_that_holds_every_chunk() {
    let Staged {
        mut bob,
        alice_final,
        ..
    } = bob_verifying_with_alice_final_withheld();

    let state = bob.process_scanned_qr(&alice_final);

    assert_eq!(state, ProtocolState::Finalized);
    assert_eq!(bob.get_received_data().as_deref(), Some(ALICE_CARD));
}

// @internal
#[test]
fn the_final_frame_is_the_size_of_the_opening_frame() {
    let Staged { alice_final, .. } = bob_verifying_with_alice_final_withheld();

    // prefix 4, header 8, session id 24, reveal key 48, tag 48
    assert_eq!(alice_final.len(), 132);
    let code = qrcode::QrCode::with_error_correction_level(&alice_final, qrcode::EcLevel::M)
        .expect("frame encodes");
    assert_eq!(code.width(), 41);
}

// @internal
#[test]
fn the_final_frame_confirms_the_card_and_both_session_ids() {
    let staged = bob_verifying_with_alice_final_withheld();
    let (alice, bob) = (&staged.alice, &staged.bob);

    let Ok(StageQr::Final {
        session_id, tag, ..
    }) = parse_qr(&staged.alice_final)
    else {
        panic!("not a final frame");
    };

    assert_eq!(session_id, alice.session_id());
    assert_eq!(
        tag,
        alice_tag(&staged, TagLabel::Confirm, ALICE_CARD, bob.session_id())
    );
}

// @internal
#[test]
fn a_confirm_whose_tag_covers_another_card_is_ignored() {
    let staged = bob_verifying_with_alice_final_withheld();
    let forged = qr_codec::format_final_qr(
        &staged.alice.session_id(),
        &reveal_key_of(&staged.alice_final),
        &alice_tag(
            &staged,
            TagLabel::Confirm,
            b"not Alice's card",
            staged.bob.session_id(),
        ),
    );
    let alice_final = staged.alice_final.clone();
    let mut bob = staged.bob;

    bob.process_scanned_qr(&forged);

    assert_eq!(
        bob.get_state(),
        ProtocolState::Confirming,
        "ignored, as a misread is"
    );
    assert_eq!(bob.get_received_data(), None);
    assert_eq!(
        bob.process_scanned_qr(&alice_final),
        ProtocolState::Finalized,
        "Alice's real confirm still finalizes"
    );
}

// @internal
#[test]
fn a_confirm_whose_tag_covers_other_session_ids_is_ignored() {
    let staged = bob_verifying_with_alice_final_withheld();
    let forged = qr_codec::format_final_qr(
        &staged.alice.session_id(),
        &reveal_key_of(&staged.alice_final),
        &alice_tag(&staged, TagLabel::Confirm, ALICE_CARD, [9u8; 16]),
    );
    let alice_final = staged.alice_final.clone();
    let mut bob = staged.bob;

    bob.process_scanned_qr(&forged);

    assert_eq!(
        bob.get_state(),
        ProtocolState::Confirming,
        "ignored, as a misread is"
    );
    assert_eq!(bob.get_received_data(), None);
    assert_eq!(
        bob.process_scanned_qr(&alice_final),
        ProtocolState::Finalized,
        "Alice's real confirm still finalizes"
    );
}

// @internal
#[test]
fn a_final_frame_with_a_wrong_reveal_key_fails_closed() {
    let staged = bob_verifying_with_alice_final_withheld();
    let forged = qr_codec::format_final_qr(
        &staged.alice.session_id(),
        &[0x5Au8; 32],
        &alice_tag(
            &staged,
            TagLabel::Confirm,
            ALICE_CARD,
            staged.bob.session_id(),
        ),
    );
    let mut bob = staged.bob;

    bob.process_scanned_qr(&forged);

    assert!(
        matches!(bob.get_state(), ProtocolState::Failed(_)),
        "got {:?}",
        bob.get_state()
    );
    assert_eq!(bob.get_received_data(), None);
}

// @internal
#[test]
fn a_final_frame_from_another_session_changes_nothing() {
    let Staged {
        mut bob,
        alice_final,
        ..
    } = bob_verifying_with_alice_final_withheld();
    let stranger = [0x77u8; 16];
    let foreign = qr_codec::format_final_qr(&stranger, &reveal_key_of(&alice_final), &[0x11; 32]);

    bob.process_scanned_qr(&foreign);

    assert_eq!(bob.get_state(), ProtocolState::Verifying);
    // The real one still completes afterwards.
    assert_eq!(
        bob.process_scanned_qr(&alice_final),
        ProtocolState::Finalized
    );
}

/// Bob holds all of Alice's chunks but has not seen her ACK for his own,
/// so he is still Transferring; Alice has reached Verifying and shown her
/// reveal.
fn bob_still_transferring_with_all_of_alices_chunks() -> Staged {
    let mut alice = MultiStageSession::new(ALICE_CARD.to_vec());
    let mut bob = MultiStageSession::new(vec![0xB2; 400]);
    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);
    // First Bob reads Alice until he holds all her chunks; she has read
    // none of his yet, so her frames ACK nothing.
    for _ in 0..200 {
        if let Some(aq) = alice.get_display_qr() {
            bob.process_scanned_qr(&aq.data);
        }
        if let ProtocolState::Transferring {
            chunks_received,
            peer_chunks_total,
            ..
        } = bob.get_state()
            && peer_chunks_total > 0
            && chunks_received == peer_chunks_total
        {
            break;
        }
    }
    // Then Alice reads Bob until she reaches Verifying and shows her
    // reveal; Bob reads nothing more of hers.
    let mut alice_reveal = None;
    for _ in 0..400 {
        if let Some(bq) = bob.get_display_qr() {
            alice.process_scanned_qr(&bq.data);
        }
        if let Some(aq) = alice.get_display_qr()
            && aq.data.starts_with("RVL4")
        {
            alice_reveal = Some(aq.data);
            break;
        }
    }
    let alice_final = alice_reveal.expect("Alice showed her reveal");
    assert!(
        matches!(bob.get_state(), ProtocolState::Transferring { .. }),
        "precondition: Bob still transferring, got {:?}",
        bob.get_state()
    );

    Staged {
        alice,
        bob,
        alice_final,
        alice_init: ai.data,
        bob_init: bi.data,
    }
}

fn revealed_key(frame: &str) -> [u8; 32] {
    match parse_qr(frame) {
        Ok(StageQr::Reveal { reveal_key, .. } | StageQr::Final { reveal_key, .. }) => reveal_key,
        other => panic!("no reveal key in {other:?}"),
    }
}

/// Each phone reads the other's next frame until Bob finalizes.
fn run_until_bob_finalizes(alice: &mut MultiStageSession, bob: &mut MultiStageSession) {
    for _ in 0..200 {
        if let Some(bq) = bob.get_display_qr() {
            alice.process_scanned_qr(&bq.data);
        }
        if let Some(aq) = alice.get_display_qr() {
            bob.process_scanned_qr(&aq.data);
        }
        if bob.get_state() == ProtocolState::Finalized {
            return;
        }
    }
    panic!("Bob never finalized: {:?}", bob.get_state());
}

// @internal
#[test]
fn a_reveal_read_while_still_transferring_opens_the_card_once_shown() {
    let Staged {
        mut alice,
        mut bob,
        alice_final: alice_reveal,
        ..
    } = bob_still_transferring_with_all_of_alices_chunks();

    bob.process_scanned_qr(&alice_reveal);
    assert_ne!(bob.get_state(), ProtocolState::Confirming);
    // Showing his next frame is when Bob opens the card with the kept key.
    let _ = bob.get_display_qr();
    assert_eq!(bob.get_state(), ProtocolState::Confirming);

    run_until_bob_finalizes(&mut alice, &mut bob);
    assert_eq!(bob.get_received_data().as_deref(), Some(ALICE_CARD));
}

/// A peer about to re-handshake still shows confirms bound to its old
/// partner (ADR-071). Such a tag, kept from before Confirming, must not
/// fail the exchange once the card opens; it is dropped, and the peer's
/// next confirm decides.
// @internal
#[test]
fn a_tag_kept_from_before_confirming_that_does_not_match_is_dropped_not_fatal() {
    let Staged {
        mut alice,
        mut bob,
        alice_final: alice_reveal,
        ..
    } = bob_still_transferring_with_all_of_alices_chunks();
    let bound_elsewhere = qr_codec::format_final_qr(
        &alice.session_id(),
        &revealed_key(&alice_reveal),
        &[0x42; 32],
    );

    bob.process_scanned_qr(&bound_elsewhere);
    let _ = bob.get_display_qr();
    assert_eq!(
        bob.get_state(),
        ProtocolState::Confirming,
        "the card opened; the stale tag neither finalized nor failed"
    );

    run_until_bob_finalizes(&mut alice, &mut bob);
    assert_eq!(bob.get_received_data().as_deref(), Some(ALICE_CARD));
}

// @internal
#[test]
fn a_done_tag_shown_as_a_confirm_is_ignored() {
    let staged = bob_verifying_with_alice_final_withheld();
    let relabelled = qr_codec::format_final_qr(
        &staged.alice.session_id(),
        &reveal_key_of(&staged.alice_final),
        &alice_tag(&staged, TagLabel::Done, ALICE_CARD, staged.bob.session_id()),
    );
    let alice_final = staged.alice_final.clone();
    let mut bob = staged.bob;

    bob.process_scanned_qr(&relabelled);

    assert_eq!(
        bob.get_state(),
        ProtocolState::Confirming,
        "ignored, as a misread is"
    );
    assert_eq!(bob.get_received_data(), None);
    assert_eq!(
        bob.process_scanned_qr(&alice_final),
        ProtocolState::Finalized,
        "Alice's real confirm still finalizes"
    );
}

// @internal
#[test]
fn a_session_still_recording_a_shake_holds_the_final_frame_until_it_stops() {
    let Staged {
        mut bob,
        alice_final,
        ..
    } = bob_verifying_with_alice_final_withheld();
    bob.set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();

    bob.process_scanned_qr(&alice_final);
    assert_eq!(
        bob.get_state(),
        ProtocolState::Confirming,
        "a shake still recording needs Confirming to swap its envelope"
    );

    bob.set_accel_proximity(AccelerometerProximityState::Confirmed)
        .unwrap();
    let _ = bob.get_display_qr();
    assert_eq!(
        bob.get_state(),
        ProtocolState::Finalized,
        "once the shake stops, the kept confirm completes the exchange"
    );
}

// @internal
#[test]
fn a_session_shows_its_confirm_only_after_its_checks_and_done_only_once_saved() {
    let Staged {
        mut bob,
        alice_final,
        ..
    } = bob_verifying_with_alice_final_withheld();
    let mut note = |session: &mut MultiStageSession| {
        let mut shown = std::collections::BTreeSet::new();
        for _ in 0..60 {
            if let Some(frame) = session.get_display_qr() {
                shown.insert(frame.data[..4].to_string());
            }
        }
        shown
    };

    let verifying = note(&mut bob);
    bob.process_scanned_qr(&alice_final);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    let finalized = note(&mut bob);
    bob.mark_saved();
    let saved = note(&mut bob);

    assert!(verifying.contains("RVL4"), "{verifying:?}");
    assert!(
        verifying
            .iter()
            .all(|p| ["RVL4", "DAT4", "SHK3"].contains(&p.as_str())),
        "no confirm before the checks: {verifying:?}"
    );
    assert!(finalized.contains("CNF4"), "{finalized:?}");
    assert!(
        !finalized.contains("DON4"),
        "no done before saving: {finalized:?}"
    );
    assert!(saved.contains("DON4"), "{saved:?}");
}
