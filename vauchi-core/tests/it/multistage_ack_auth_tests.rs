// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A DATA frame's ACK bitmap counts only if the frame authenticates
//! (vauchi/private#676). The bitmap decides which of our chunks we stop
//! showing; a misread or forged one that claims chunks the peer lacks used
//! to stall the exchange, and it was applied even when the frame's chunk
//! failed to decrypt.

use vauchi_core::exchange::multistage::qr_codec::{
    QrCodecError, StageQr, format_data_qr, parse_qr,
};
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ChunkBitmap;

use proptest::prelude::*;

fn three_chunk_card(tag: u8) -> Vec<u8> {
    // 62 bytes seal to 102, three chunks of 34.
    vec![tag; 62]
}

/// Two sessions that have scanned each other's opening frames.
fn discovered_pair() -> (MultiStageSession, MultiStageSession) {
    let mut ours = MultiStageSession::new(three_chunk_card(0xA1));
    let mut peer = MultiStageSession::new(three_chunk_card(0xB2));
    let oi = ours.get_display_qr().expect("advertises");
    let pi = peer.get_display_qr().expect("advertises");
    ours.process_scanned_qr(&pi.data);
    peer.process_scanned_qr(&oi.data);
    (ours, peer)
}

/// The chunk indices `session` shows over enough frames to cover a pass.
fn chunks_shown(session: &mut MultiStageSession) -> Vec<u16> {
    let mut shown = Vec::new();
    for _ in 0..60 {
        if let Some(qr) = session.get_display_qr()
            && let Ok(StageQr::Data { chunk_idx, .. }) = parse_qr(&qr.data)
            && !shown.contains(&chunk_idx)
        {
            shown.push(chunk_idx);
        }
    }
    shown.sort_unstable();
    shown
}

/// A genuine DATA frame from `peer`, rebuilt with `ack` in place of the
/// bitmap it carried and everything else unchanged.
fn genuine_frame_with_ack(peer: &mut MultiStageSession, ack: &ChunkBitmap) -> String {
    let frame = loop {
        let qr = peer.get_display_qr().expect("peer shows a frame");
        if let Ok(parsed @ StageQr::Data { .. }) = parse_qr(&qr.data) {
            break parsed;
        }
    };
    let StageQr::Data {
        chunk_idx,
        chunk_total,
        payload,
        ..
    } = frame
    else {
        unreachable!()
    };
    format_data_qr(
        &peer.session_id(),
        chunk_idx,
        chunk_total,
        &ack.to_bytes(),
        &payload,
    )
}

// @internal
#[test]
fn an_ack_altered_on_a_genuine_frame_is_not_applied() {
    let (mut ours, mut peer) = discovered_pair();
    let mut forged = ChunkBitmap::new(3);
    for idx in 0..3 {
        forged.mark_received(idx);
    }

    ours.process_scanned_qr(&genuine_frame_with_ack(&mut peer, &forged));

    assert_eq!(
        chunks_shown(&mut ours),
        vec![0, 1, 2],
        "a bitmap claiming every chunk, put on a frame it did not come with, \
         must not stop us showing chunks the peer never received"
    );
}

// @internal
#[test]
fn an_ack_on_a_frame_whose_chunk_does_not_decrypt_is_not_applied() {
    let (mut ours, peer) = discovered_pair();
    let mut held = ChunkBitmap::new(3);
    held.mark_received(2);

    ours.process_scanned_qr(&format_data_qr(
        &peer.session_id(),
        0,
        3,
        &held.to_bytes(),
        b"not-a-chunk-the-peer-encrypted",
    ));

    assert_eq!(chunks_shown(&mut ours), vec![0, 1, 2]);
}

/// The fields of one genuine DATA frame from `peer`.
struct GenuineFrame {
    session_id: [u8; 16],
    chunk_idx: u16,
    chunk_total: u16,
    ack: Vec<u8>,
    payload: Vec<u8>,
}

fn genuine_frame(peer: &mut MultiStageSession) -> GenuineFrame {
    loop {
        let qr = peer.get_display_qr().expect("peer shows a frame");
        if let Ok(StageQr::Data {
            chunk_idx,
            chunk_total,
            ack_bitmap,
            payload,
            ..
        }) = parse_qr(&qr.data)
        {
            return GenuineFrame {
                session_id: peer.session_id(),
                chunk_idx,
                chunk_total,
                ack: ack_bitmap,
                payload,
            };
        }
    }
}

/// Feeds `ours`' DATA frames to `peer` until the peer has every chunk.
fn peer_receives_all_of(ours: &mut MultiStageSession, peer: &mut MultiStageSession) {
    for _ in 0..60 {
        let qr = ours.get_display_qr().expect("we show a frame");
        peer.process_scanned_qr(&qr.data);
    }
}

// Positive control: without it, an ACK that is never applied at all would
// pass every "not applied" test in this file.
// @internal
#[test]
fn a_genuine_frames_ack_is_applied() {
    let (mut ours, mut peer) = discovered_pair();
    peer_receives_all_of(&mut ours, &mut peer);
    let frame = genuine_frame(&mut peer);

    ours.process_scanned_qr(&format_data_qr(
        &frame.session_id,
        frame.chunk_idx,
        frame.chunk_total,
        &frame.ack,
        &frame.payload,
    ));

    // With every chunk acked, chunk 0 alone keeps going out, as the carrier
    // for our own ACK bitmap (`get_data_chunk_qr`).
    assert_eq!(chunks_shown(&mut ours), vec![0]);
}

/// Which field of a genuine frame a forgery changes.
#[derive(Debug, Clone)]
enum Alteration {
    SessionId([u8; 16]),
    ChunkIdx(u16),
    ChunkTotal(u16),
    Ack(Vec<u8>),
}

fn alteration() -> impl Strategy<Value = Alteration> {
    prop_oneof![
        any::<[u8; 16]>().prop_map(Alteration::SessionId),
        (0u16..8).prop_map(Alteration::ChunkIdx),
        (1u16..8).prop_map(Alteration::ChunkTotal),
        // Truncated, empty, all-ones and over-long bitmaps alike.
        proptest::collection::vec(any::<u8>(), 0..4).prop_map(Alteration::Ack),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // Any one changed field of a genuine frame fails authentication, so the
    // all-chunks ACK it carries is never applied (CC-04, CC-14).
    // @internal
    #[test]
    fn a_forged_ack_on_any_altered_frame_is_not_applied(change in alteration()) {
        let (mut ours, mut peer) = discovered_pair();
        let frame = genuine_frame(&mut peer);
        let mut all_acked = ChunkBitmap::new(3);
        for idx in 0..3 {
            all_acked.mark_received(idx);
        }
        let (mut session_id, mut chunk_idx, mut chunk_total, mut ack) =
            (frame.session_id, frame.chunk_idx, frame.chunk_total, all_acked.to_bytes());
        match change {
            Alteration::SessionId(v) => session_id = v,
            Alteration::ChunkIdx(v) => chunk_idx = v,
            Alteration::ChunkTotal(v) => chunk_total = v,
            Alteration::Ack(v) => ack = v,
        }
        let unaltered = session_id == frame.session_id
            && chunk_idx == frame.chunk_idx
            && chunk_total == frame.chunk_total
            && ack == frame.ack;
        prop_assume!(!unaltered);

        ours.process_scanned_qr(&format_data_qr(
            &session_id, chunk_idx, chunk_total, &ack, &frame.payload,
        ));

        prop_assert_eq!(chunks_shown(&mut ours), vec![0, 1, 2]);
    }
}

// @internal
#[test]
fn a_dat3_frame_is_an_old_format() {
    assert!(matches!(
        parse_qr("DAT3AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
        Err(QrCodecError::OldFormat)
    ));
}
