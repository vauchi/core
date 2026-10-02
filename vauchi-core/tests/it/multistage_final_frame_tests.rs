// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The ending is one frame: the reveal key and one value that confirms the
//! card and both session ids. One read of it takes a peer that holds every
//! chunk to Finalized, whatever finalization state it is in (design D5).

use sha2::{Digest, Sha256};
use vauchi_core::exchange::multistage::qr_codec::{self, StageQr, parse_qr};
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::{AccelerometerProximityState, ProtocolState};

const ALICE_CARD: &[u8] = b"Alice (iPhone)";
const BOB_CARD: &[u8] = b"Bob (Pixel)";

struct Staged {
    alice: MultiStageSession,
    bob: MultiStageSession,
    alice_final: String,
}

fn is_final(frame: &str) -> bool {
    frame.starts_with("FIN3")
}

/// Drives both sessions until Bob holds every chunk and waits in Verifying,
/// with Alice's final frame captured but never shown to him.
fn bob_verifying_with_alice_final_withheld() -> Staged {
    let mut alice = MultiStageSession::new(ALICE_CARD.to_vec());
    let mut bob = MultiStageSession::new(BOB_CARD.to_vec());

    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    let mut alice_final = None;
    for _ in 0..500 {
        if let Some(aq) = alice.get_display_qr() {
            if is_final(&aq.data) {
                alice_final = Some(aq.data);
            } else {
                bob.process_scanned_qr(&aq.data);
            }
        }
        // Bob's own final frame stays with him, so Alice cannot finish and
        // keeps showing hers.
        if let Some(bq) = bob.get_display_qr()
            && !is_final(&bq.data)
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
        alice_final: alice_final.expect("Alice never showed a final frame"),
    }
}

/// The value Alice's final frame confirms: her card and both session ids.
fn final_tag(card: &[u8], one: [u8; 16], other: [u8; 16]) -> [u8; 32] {
    let card_hash: [u8; 32] = Sha256::digest(card).into();
    let (first, second) = if one <= other {
        (one, other)
    } else {
        (other, one)
    };
    let ready_hash: [u8; 32] = Sha256::digest([first, second].concat()).into();
    Sha256::digest([card_hash, ready_hash].concat()).into()
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
    let Staged {
        alice,
        bob,
        alice_final,
    } = bob_verifying_with_alice_final_withheld();

    let Ok(StageQr::Final {
        session_id, tag, ..
    }) = parse_qr(&alice_final)
    else {
        panic!("not a final frame");
    };

    assert_eq!(session_id, alice.session_id());
    assert_eq!(
        tag,
        final_tag(ALICE_CARD, alice.session_id(), bob.session_id())
    );
}

// @internal
#[test]
fn a_final_frame_confirming_another_card_fails_closed() {
    let Staged {
        alice,
        mut bob,
        alice_final,
    } = bob_verifying_with_alice_final_withheld();
    let forged = qr_codec::format_final_qr(
        &alice.session_id(),
        &reveal_key_of(&alice_final),
        &final_tag(b"not Alice's card", alice.session_id(), bob.session_id()),
    );

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
fn a_final_frame_confirming_other_session_ids_fails_closed() {
    let Staged {
        alice,
        mut bob,
        alice_final,
    } = bob_verifying_with_alice_final_withheld();
    let forged = qr_codec::format_final_qr(
        &alice.session_id(),
        &reveal_key_of(&alice_final),
        &final_tag(ALICE_CARD, alice.session_id(), [9u8; 16]),
    );

    bob.process_scanned_qr(&forged);

    assert!(
        matches!(bob.get_state(), ProtocolState::Failed(_)),
        "got {:?}",
        bob.get_state()
    );
}

// @internal
#[test]
fn a_final_frame_with_a_wrong_reveal_key_fails_closed() {
    let Staged { alice, mut bob, .. } = bob_verifying_with_alice_final_withheld();
    let forged = qr_codec::format_final_qr(
        &alice.session_id(),
        &[0x5Au8; 32],
        &final_tag(ALICE_CARD, alice.session_id(), bob.session_id()),
    );

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
    let foreign = qr_codec::format_final_qr(
        &stranger,
        &reveal_key_of(&alice_final),
        &final_tag(ALICE_CARD, stranger, bob.session_id()),
    );

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
/// final frame.
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
    // Then Alice reads Bob until she reaches Verifying and shows her final
    // frame; Bob reads nothing more of hers.
    let mut alice_final = None;
    for _ in 0..400 {
        if let Some(bq) = bob.get_display_qr()
            && !is_final(&bq.data)
        {
            alice.process_scanned_qr(&bq.data);
        }
        if let Some(aq) = alice.get_display_qr()
            && is_final(&aq.data)
        {
            alice_final = Some(aq.data);
            break;
        }
    }
    let alice_final = alice_final.expect("Alice showed her final frame");
    assert!(
        matches!(bob.get_state(), ProtocolState::Transferring { .. }),
        "precondition: Bob still transferring, got {:?}",
        bob.get_state()
    );

    Staged {
        alice,
        bob,
        alice_final,
    }
}

// @internal
#[test]
fn a_final_frame_read_while_still_transferring_finalizes_once_the_card_opens() {
    let Staged {
        mut bob,
        alice_final,
        ..
    } = bob_still_transferring_with_all_of_alices_chunks();

    bob.process_scanned_qr(&alice_final);
    assert_ne!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_received_data(), None);
    // Showing his next frame is when Bob opens the card with the kept key;
    // the tag read with it completes the exchange without a second read.
    let _ = bob.get_display_qr();

    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    assert_eq!(bob.get_received_data().as_deref(), Some(ALICE_CARD));
}

/// A peer about to re-handshake still shows final frames bound to its old
/// partner (ADR-071). Such a tag, kept from before Confirming, must not
/// fail the exchange once the card opens; it is dropped, and the peer's
/// next final frame decides.
// @internal
#[test]
fn a_tag_kept_from_before_confirming_that_does_not_match_is_dropped_not_fatal() {
    let Staged {
        alice,
        mut bob,
        alice_final,
    } = bob_still_transferring_with_all_of_alices_chunks();
    let bound_elsewhere = qr_codec::format_final_qr(
        &alice.session_id(),
        &reveal_key_of(&alice_final),
        &final_tag(ALICE_CARD, alice.session_id(), [9u8; 16]),
    );

    bob.process_scanned_qr(&bound_elsewhere);
    let _ = bob.get_display_qr();
    assert_eq!(
        bob.get_state(),
        ProtocolState::Confirming,
        "the card opened; the stale tag neither finalized nor failed"
    );

    assert_eq!(
        bob.process_scanned_qr(&alice_final),
        ProtocolState::Finalized
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
        "once the shake stops, the kept final frame completes the exchange"
    );
}

// @internal
#[test]
fn from_verifying_on_a_session_shows_only_data_shake_and_final_frames() {
    let Staged {
        mut bob,
        alice_final,
        ..
    } = bob_verifying_with_alice_final_withheld();
    let mut shown = std::collections::BTreeSet::new();
    let mut note = |session: &mut MultiStageSession| {
        for _ in 0..60 {
            if let Some(frame) = session.get_display_qr() {
                shown.insert(frame.data[..4].to_string());
            }
        }
    };

    note(&mut bob);
    bob.process_scanned_qr(&alice_final);
    assert_eq!(bob.get_state(), ProtocolState::Finalized);
    note(&mut bob);

    assert!(shown.contains("FIN3"), "{shown:?}");
    assert!(
        shown
            .iter()
            .all(|p| ["FIN3", "DAT3", "SHK3"].contains(&p.as_str())),
        "{shown:?}"
    );
}
