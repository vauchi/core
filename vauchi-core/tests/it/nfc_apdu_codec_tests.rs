// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wire-level tests for the core-owned NFC APDU codec
//! (`vauchi_core::exchange::nfc_apdu`).
//!
//! The byte vectors are lifted verbatim from the shells that spoke the
//! protocol before framing moved into core (RG-11): the Linux PC/SC
//! reader's SELECT, the iOS/Android readers' `00 E0 00 00 Lc <data>`
//! EXCHANGE, and the Android HCE responder's status words. Any change
//! here is a wire break for every shell at once.
//!
//! - **Golden bytes** for SELECT, EXCHANGE, and the status words.
//! - **Adversarial** decoder inputs (CC-14 / DC-01): truncated, wrong
//!   AID, mismatched `Lc`, oversized frames.
//! - **Property-based** chunk round-trips (CC-04).

use proptest::prelude::*;
use vauchi_core::exchange::nfc_apdu::{
    AID, ApduCommand, ApduError, ChainReassembler, MAX_APDU_DATA, MAX_EXCHANGE_PAYLOAD,
    SW_AID_NOT_FOUND, SW_CONDITIONS_NOT_SATISFIED, SW_SUCCESS, SW_WRONG_DATA, assemble_chain,
    build_select, decode_command, decode_response, frame_command, frame_exchange, frame_response,
    status_response,
};

/// `00 A4 04 00 07 F0 56 41 55 43 48 49` — what `linux-gtk` transmits and
/// what Android's `VauchiHceService.isSelectAid` accepts.
const SELECT_ON_THE_WIRE: [u8; 12] = [
    0x00, 0xA4, 0x04, 0x00, 0x07, 0xF0, 0x56, 0x41, 0x55, 0x43, 0x48, 0x49,
];

// ============================================================
// Constants
// ============================================================

// @internal
#[test]
fn aid_matches_the_shells_registered_seven_byte_aid() {
    assert_eq!(AID, &[0xF0, 0x56, 0x41, 0x55, 0x43, 0x48, 0x49]);
}

// @internal
#[test]
fn status_words_match_iso7816_values() {
    assert_eq!(SW_SUCCESS, [0x90, 0x00]);
    assert_eq!(SW_AID_NOT_FOUND, [0x6A, 0x82]);
    assert_eq!(SW_CONDITIONS_NOT_SATISFIED, [0x69, 0x85]);
    assert_eq!(SW_WRONG_DATA, [0x6A, 0x80]);
}

// @internal
#[test]
fn ceilings_are_one_short_lc_and_the_dc03_total() {
    assert_eq!(MAX_APDU_DATA, 255);
    assert_eq!(MAX_EXCHANGE_PAYLOAD, 4096);
}

// ============================================================
// Framing
// ============================================================

// @internal
#[test]
fn select_apdu_matches_the_wire_bytes() {
    assert_eq!(build_select(), SELECT_ON_THE_WIRE.to_vec());
}

// @internal
#[test]
fn frame_command_emits_select_then_one_short_exchange_apdu() {
    let payload = [0xAA; 174];
    let apdus = frame_command(&payload).expect("174 bytes fits one short APDU");

    assert_eq!(apdus.len(), 2);
    assert_eq!(apdus[0], SELECT_ON_THE_WIRE.to_vec());
    let mut expected = vec![0x00, 0xE0, 0x00, 0x00, 0xAE];
    expected.extend_from_slice(&payload);
    assert_eq!(apdus[1], expected);
}

// @internal
#[test]
fn frame_exchange_omits_select() {
    let apdus = frame_exchange(&[0x01, 0x02, 0x03]).expect("must frame");
    assert_eq!(
        apdus,
        vec![vec![0x00, 0xE0, 0x00, 0x00, 0x03, 0x01, 0x02, 0x03]]
    );
}

// @internal
#[test]
fn frame_exchange_with_empty_payload_emits_lc_zero() {
    let apdus = frame_exchange(&[]).expect("must frame");
    assert_eq!(apdus, vec![vec![0x00, 0xE0, 0x00, 0x00, 0x00]]);
}

// @internal
#[test]
fn frame_exchange_at_chunk_boundary_stays_a_single_apdu() {
    let payload = vec![0xBB; MAX_APDU_DATA];
    let apdus = frame_exchange(&payload).expect("must frame");
    assert_eq!(apdus.len(), 1);
    assert_eq!(apdus[0][0], 0x00, "final chunk has no chaining bit");
    assert_eq!(apdus[0][4], 0xFF, "Lc = 255");
}

// @internal
#[test]
fn frame_exchange_one_past_chunk_boundary_chains_two_apdus() {
    let payload: Vec<u8> = (0..=255u8).collect();
    let apdus = frame_exchange(&payload).expect("must frame");

    assert_eq!(apdus.len(), 2);
    assert_eq!(&apdus[0][..5], &[0x10, 0xE0, 0x00, 0x00, 0xFF]);
    assert_eq!(&apdus[0][5..], &payload[..255]);
    assert_eq!(apdus[1], vec![0x00, 0xE0, 0x00, 0x00, 0x01, 0xFF]);
}

// @internal
#[test]
fn frame_exchange_rejects_payload_over_the_ceiling() {
    let payload = vec![0u8; MAX_EXCHANGE_PAYLOAD + 1];
    assert_eq!(
        frame_exchange(&payload),
        Err(ApduError::Oversized {
            len: MAX_EXCHANGE_PAYLOAD + 1,
            max: MAX_EXCHANGE_PAYLOAD,
        })
    );
    assert_eq!(
        frame_command(&payload),
        Err(ApduError::Oversized {
            len: MAX_EXCHANGE_PAYLOAD + 1,
            max: MAX_EXCHANGE_PAYLOAD,
        })
    );
}

// @internal
#[test]
fn frame_response_appends_success_status_word() {
    assert_eq!(
        frame_response(&[0xDE, 0xAD]).expect("must frame"),
        vec![0xDE, 0xAD, 0x90, 0x00]
    );
}

// @internal
#[test]
fn frame_response_rejects_data_over_the_ceiling() {
    let data = vec![0u8; MAX_EXCHANGE_PAYLOAD + 1];
    assert_eq!(
        frame_response(&data),
        Err(ApduError::Oversized {
            len: MAX_EXCHANGE_PAYLOAD + 1,
            max: MAX_EXCHANGE_PAYLOAD,
        })
    );
}

// @internal
#[test]
fn status_response_is_the_bare_status_word() {
    assert_eq!(status_response(SW_SUCCESS), vec![0x90, 0x00]);
    assert_eq!(status_response(SW_AID_NOT_FOUND), vec![0x6A, 0x82]);
}

// ============================================================
// decode_response
// ============================================================

// @internal
#[test]
fn decode_response_strips_success_status_word() {
    let bytes = [0xDE, 0xAD, 0xBE, 0xEF, 0x90, 0x00];
    assert_eq!(decode_response(&bytes), Ok(vec![0xDE, 0xAD, 0xBE, 0xEF]));
}

// @internal
#[test]
fn decode_response_with_only_success_yields_empty_data() {
    assert_eq!(decode_response(&[0x90, 0x00]), Ok(Vec::new()));
}

// @internal
#[test]
fn decode_response_maps_aid_not_found() {
    assert_eq!(decode_response(&[0x6A, 0x82]), Err(ApduError::AidNotFound));
}

// @internal
#[test]
fn decode_response_maps_conditions_not_satisfied() {
    assert_eq!(
        decode_response(&[0x69, 0x85]),
        Err(ApduError::ConditionsNotSatisfied)
    );
}

// @internal
#[test]
fn decode_response_maps_wrong_data() {
    assert_eq!(decode_response(&[0x6A, 0x80]), Err(ApduError::WrongData));
}

// @internal
#[test]
fn decode_response_reports_any_other_status_word() {
    assert_eq!(
        decode_response(&[0x01, 0x6F, 0x00]),
        Err(ApduError::UnexpectedStatus(0x6F, 0x00))
    );
}

// @internal
#[test]
fn decode_response_rejects_truncated_input() {
    assert_eq!(decode_response(&[]), Err(ApduError::Truncated));
    assert_eq!(decode_response(&[0x90]), Err(ApduError::Truncated));
}

// @internal
#[test]
fn decode_response_rejects_oversized_data() {
    let mut bytes = vec![0u8; MAX_EXCHANGE_PAYLOAD + 1];
    bytes.extend_from_slice(&SW_SUCCESS);
    assert_eq!(
        decode_response(&bytes),
        Err(ApduError::Oversized {
            len: MAX_EXCHANGE_PAYLOAD + 1,
            max: MAX_EXCHANGE_PAYLOAD,
        })
    );
}

// ============================================================
// decode_command
// ============================================================

// @internal
#[test]
fn decode_command_recognizes_the_wire_select() {
    assert_eq!(decode_command(&SELECT_ON_THE_WIRE), Ok(ApduCommand::Select));
}

// @internal
#[test]
fn decode_command_rejects_select_for_another_aid() {
    let mut bytes = SELECT_ON_THE_WIRE;
    bytes[11] = 0x4A;
    assert_eq!(decode_command(&bytes), Err(ApduError::AidNotFound));
}

// @internal
#[test]
fn decode_command_extracts_short_exchange_data() {
    let bytes = [0x00, 0xE0, 0x00, 0x00, 0x03, 0x01, 0x02, 0x03];
    assert_eq!(
        decode_command(&bytes),
        Ok(ApduCommand::Exchange {
            data: vec![0x01, 0x02, 0x03],
            more: false,
        })
    );
}

// @internal
#[test]
fn decode_command_tolerates_linux_reader_cla_and_trailing_le() {
    // `80 E0 00 00 03 01 02 03 00` — the PC/SC reader's EXCHANGE.
    let bytes = [0x80, 0xE0, 0x00, 0x00, 0x03, 0x01, 0x02, 0x03, 0x00];
    assert_eq!(
        decode_command(&bytes),
        Ok(ApduCommand::Exchange {
            data: vec![0x01, 0x02, 0x03],
            more: false,
        })
    );
}

// @internal
#[test]
fn decode_command_flags_chained_chunk_as_more() {
    let bytes = [0x10, 0xE0, 0x00, 0x00, 0x01, 0x42];
    assert_eq!(
        decode_command(&bytes),
        Ok(ApduCommand::Exchange {
            data: vec![0x42],
            more: true,
        })
    );
}

// @internal
#[test]
fn decode_command_extracts_extended_length_data() {
    // Android's reader uses `00 E0 00 00 00 LL LL <data>` above 255 bytes.
    let data = vec![0x5A; 256];
    let mut bytes = vec![0x00, 0xE0, 0x00, 0x00, 0x00, 0x01, 0x00];
    bytes.extend_from_slice(&data);
    assert_eq!(
        decode_command(&bytes),
        Ok(ApduCommand::Exchange { data, more: false })
    );
}

// @internal
#[test]
fn decode_command_reports_unknown_instruction() {
    let bytes = [0x00, 0xE2, 0x00, 0x00, 0x01, 0x42];
    assert_eq!(
        decode_command(&bytes),
        Ok(ApduCommand::Unknown { ins: 0xE2 })
    );
}

// @internal
#[test]
fn decode_command_rejects_header_shorter_than_four_bytes() {
    assert_eq!(decode_command(&[]), Err(ApduError::MalformedCommand));
    assert_eq!(
        decode_command(&[0x00, 0xE0, 0x00]),
        Err(ApduError::MalformedCommand)
    );
}

// @internal
#[test]
fn decode_command_rejects_lc_longer_than_the_frame() {
    let bytes = [0x00, 0xE0, 0x00, 0x00, 0x05, 0x01];
    assert_eq!(decode_command(&bytes), Err(ApduError::MalformedCommand));
}

// @internal
#[test]
fn decode_command_rejects_extended_lc_longer_than_the_frame() {
    let bytes = [0x00, 0xE0, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01];
    assert_eq!(decode_command(&bytes), Err(ApduError::MalformedCommand));
}

// @internal
#[test]
fn decode_command_rejects_extended_data_over_the_ceiling() {
    let len = MAX_EXCHANGE_PAYLOAD + 1;
    let mut bytes = vec![0x00, 0xE0, 0x00, 0x00, 0x00, (len >> 8) as u8, len as u8];
    bytes.extend(std::iter::repeat_n(0u8, len));
    assert_eq!(
        decode_command(&bytes),
        Err(ApduError::Oversized {
            len,
            max: MAX_EXCHANGE_PAYLOAD,
        })
    );
}

// ============================================================
// Chain reassembly
// ============================================================

// @internal
#[test]
fn assemble_chain_round_trips_a_three_chunk_payload() {
    let payload: Vec<u8> = (0..(MAX_APDU_DATA * 2 + 50)).map(|i| i as u8).collect();
    let apdus = frame_exchange(&payload).expect("must frame");
    assert_eq!(apdus.len(), 3);
    assert_eq!(assemble_chain(&apdus), Ok(payload));
}

// @internal
#[test]
fn assemble_chain_rejects_empty_chain() {
    assert_eq!(assemble_chain(&[]), Err(ApduError::ChainBroken));
}

// @internal
#[test]
fn assemble_chain_rejects_chain_without_final_chunk() {
    let apdus = vec![vec![0x10, 0xE0, 0x00, 0x00, 0x01, 0x42]];
    assert_eq!(assemble_chain(&apdus), Err(ApduError::ChainBroken));
}

// @internal
#[test]
fn assemble_chain_rejects_select_inside_the_chain() {
    let apdus = frame_command(&[0x42]).expect("must frame");
    assert_eq!(assemble_chain(&apdus), Err(ApduError::ChainBroken));
}

// @internal
#[test]
fn reassembler_yields_payload_only_on_the_final_chunk() {
    let mut reassembler = ChainReassembler::new();
    assert_eq!(reassembler.push(&[0x01, 0x02], true), Ok(None));
    assert_eq!(
        reassembler.push(&[0x03], false),
        Ok(Some(vec![0x01, 0x02, 0x03]))
    );
    assert_eq!(
        reassembler.push(&[0x04], false),
        Ok(Some(vec![0x04])),
        "a completed chain must not leak into the next one"
    );
}

// @internal
#[test]
fn reassembler_rejects_chain_over_the_ceiling_and_resets() {
    let mut reassembler = ChainReassembler::new();
    let chunk = vec![0u8; MAX_APDU_DATA];
    let mut pushed = 0;
    let overflow = loop {
        match reassembler.push(&chunk, true) {
            Ok(None) => pushed += chunk.len(),
            Ok(Some(_)) => panic!("non-final push must never complete"),
            Err(e) => break e,
        }
    };
    assert_eq!(
        overflow,
        ApduError::Oversized {
            len: pushed + chunk.len(),
            max: MAX_EXCHANGE_PAYLOAD,
        }
    );
    assert_eq!(
        reassembler.push(&[0x01], false),
        Ok(Some(vec![0x01])),
        "an overflowed chain must be discarded, not resumed"
    );
}

// ============================================================
// Properties (CC-04)
// ============================================================

proptest! {
    /// `frame_exchange` then `assemble_chain` is the identity for every
    /// payload under the ceiling, and the chain never exceeds one short
    /// `Lc` per APDU.
    // @internal
    #[test]
    fn prop_frame_exchange_round_trips(
        payload in proptest::collection::vec(any::<u8>(), 0..=(MAX_APDU_DATA * 4))
    ) {
        let apdus = frame_exchange(&payload).expect("under the ceiling");
        for apdu in &apdus {
            prop_assert!(apdu.len() <= 5 + MAX_APDU_DATA);
        }
        prop_assert_eq!(apdus.len(), payload.len().div_ceil(MAX_APDU_DATA).max(1));
        prop_assert_eq!(assemble_chain(&apdus), Ok(payload));
    }

    /// `frame_response` then `decode_response` is the identity.
    // @internal
    #[test]
    fn prop_frame_response_round_trips(
        data in proptest::collection::vec(any::<u8>(), 0..=512)
    ) {
        let framed = frame_response(&data).expect("under the ceiling");
        prop_assert_eq!(decode_response(&framed), Ok(data));
    }

    /// The decoder never panics and only ever accepts a `SELECT` that
    /// carries exactly our AID.
    // @internal
    #[test]
    fn prop_decode_command_never_panics(
        bytes in proptest::collection::vec(any::<u8>(), 0..=300)
    ) {
        if decode_command(&bytes) == Ok(ApduCommand::Select) {
            prop_assert_eq!(bytes[1], 0xA4);
            prop_assert_eq!(bytes[2], 0x04);
            prop_assert_eq!(&bytes[5..5 + AID.len()], AID);
        }
    }
}
