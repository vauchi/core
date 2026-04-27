// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! ISO 7816 APDU Protocol Tests for `nfc_active::apdu`
//!
//! These exercise the byte-level command/response builders and parsers
//! exposed at `vauchi_core::exchange::nfc_apdu`. They are pure functions
//! with no I/O, so coverage is fully achievable from unit tests.
//!
//! - **Golden bytes** for `build_select`, `AID`, status word constants.
//! - **Header invariants** for `build_exchange_data`, `build_card_exchange`.
//! - **Adversarial** parser inputs (CC-14): truncated, empty, mismatched
//!   `Lc`, wrong INS, wrong P1, wrong AID.
//! - **Property-based** roundtrips (CC-04).

use proptest::prelude::*;
use vauchi_core::exchange::nfc_apdu::{
    AID, SW_AID_NOT_FOUND, SW_CONDITIONS_NOT_SATISFIED, SW_SUCCESS, build_card_exchange,
    build_exchange_data, build_select, is_card_exchange, is_exchange_data, is_select_vauchi,
    parse_command, parse_response,
};

// APDU instruction codes copied here so tests pin the on-the-wire values.
// If these change, the protocol changed — tests must be updated deliberately.
const INS_SELECT: u8 = 0xA4;
const INS_EXCHANGE_DATA: u8 = 0xE0;
const INS_CARD_EXCHANGE: u8 = 0xE2;

// ============================================================
// Constants
// ============================================================

// @internal
#[test]
fn aid_is_eight_bytes_with_vauchi_marker() {
    assert_eq!(AID.len(), 8);
    assert_eq!(AID, &[0xF0, 0x56, 0x41, 0x55, 0x43, 0x48, 0x49, 0x01]);
    assert_eq!(&AID[1..7], b"VAUCHI");
}

// @internal
#[test]
fn sw_success_is_9000() {
    assert_eq!(SW_SUCCESS, [0x90, 0x00]);
}

// @internal
#[test]
fn sw_aid_not_found_is_6a82() {
    assert_eq!(SW_AID_NOT_FOUND, [0x6A, 0x82]);
}

// @internal
#[test]
fn sw_conditions_not_satisfied_is_6985() {
    assert_eq!(SW_CONDITIONS_NOT_SATISFIED, [0x69, 0x85]);
}

// ============================================================
// build_select
// ============================================================

// @internal
#[test]
fn build_select_emits_iso7816_select_by_name() {
    let cmd = build_select();
    assert_eq!(cmd.len(), 5 + AID.len(), "5-byte header + AID");
    assert_eq!(cmd[0], 0x00, "CLA = 0x00");
    assert_eq!(cmd[1], INS_SELECT, "INS = SELECT (0xA4)");
    assert_eq!(cmd[2], 0x04, "P1 = select-by-name");
    assert_eq!(cmd[3], 0x00, "P2 = 0x00");
    assert_eq!(cmd[4], AID.len() as u8, "Lc = AID length");
    assert_eq!(&cmd[5..], AID);
}

// @internal
#[test]
fn build_select_round_trips_through_parse_command() {
    let cmd = build_select();
    let (ins, p1, p2, data) = parse_command(&cmd).expect("must parse");
    assert_eq!(ins, INS_SELECT);
    assert_eq!(p1, 0x04);
    assert_eq!(p2, 0x00);
    assert_eq!(data, AID);
}

// @internal
#[test]
fn build_select_is_recognized_as_select_vauchi() {
    assert!(is_select_vauchi(&build_select()));
}

// ============================================================
// build_exchange_data
// ============================================================

// @internal
#[test]
fn build_exchange_data_header_and_payload() {
    let payload = [0xAA; 174];
    let cmd = build_exchange_data(&payload);

    assert_eq!(cmd.len(), 5 + payload.len());
    assert_eq!(cmd[0], 0x00, "CLA");
    assert_eq!(cmd[1], INS_EXCHANGE_DATA, "INS = 0xE0");
    assert_eq!(cmd[2], 0x00, "P1");
    assert_eq!(cmd[3], 0x00, "P2");
    assert_eq!(cmd[4], payload.len() as u8, "Lc");
    assert_eq!(&cmd[5..], &payload);
}

// @internal
#[test]
fn build_exchange_data_with_empty_payload_emits_lc_zero() {
    let cmd = build_exchange_data(&[]);
    assert_eq!(cmd, vec![0x00, INS_EXCHANGE_DATA, 0x00, 0x00, 0x00]);
}

// @internal
#[test]
fn build_exchange_data_is_classified_correctly() {
    let cmd = build_exchange_data(&[0x01, 0x02, 0x03]);
    assert!(is_exchange_data(&cmd));
    assert!(!is_card_exchange(&cmd));
    assert!(!is_select_vauchi(&cmd));
}

// ============================================================
// build_card_exchange
// ============================================================

// @internal
#[test]
fn build_card_exchange_header_and_payload() {
    let blob = [0xCC; 200];
    let cmd = build_card_exchange(&blob);

    assert_eq!(cmd.len(), 5 + blob.len());
    assert_eq!(cmd[0], 0x00, "CLA");
    assert_eq!(cmd[1], INS_CARD_EXCHANGE, "INS = 0xE2");
    assert_eq!(cmd[2], 0x00, "P1");
    assert_eq!(cmd[3], 0x00, "P2");
    assert_eq!(cmd[4], blob.len() as u8, "Lc");
    assert_eq!(&cmd[5..], &blob);
}

// @internal
#[test]
fn build_card_exchange_with_empty_blob_emits_lc_zero() {
    let cmd = build_card_exchange(&[]);
    assert_eq!(cmd, vec![0x00, INS_CARD_EXCHANGE, 0x00, 0x00, 0x00]);
}

// @internal
#[test]
fn build_card_exchange_is_classified_correctly() {
    let cmd = build_card_exchange(&[0x10, 0x20]);
    assert!(is_card_exchange(&cmd));
    assert!(!is_exchange_data(&cmd));
    assert!(!is_select_vauchi(&cmd));
}

// ============================================================
// parse_response
// ============================================================

// @internal
#[test]
fn parse_response_splits_data_and_status_word() {
    let resp = [0xDE, 0xAD, 0xBE, 0xEF, 0x90, 0x00];
    let (data, sw) = parse_response(&resp).expect("must parse");
    assert_eq!(data, &[0xDE, 0xAD, 0xBE, 0xEF]);
    assert_eq!(sw, [0x90, 0x00]);
}

// @internal
#[test]
fn parse_response_returns_empty_data_when_only_status_word() {
    let (data, sw) = parse_response(&[0x6A, 0x82]).expect("must parse");
    assert_eq!(data, &[] as &[u8]);
    assert_eq!(sw, [0x6A, 0x82]);
}

// @internal
#[test]
fn parse_response_rejects_one_byte_input() {
    assert!(parse_response(&[0x90]).is_none());
}

// @internal
#[test]
fn parse_response_rejects_empty_input() {
    assert!(parse_response(&[]).is_none());
}

// ============================================================
// parse_command
// ============================================================

// @internal
#[test]
fn parse_command_minimum_four_byte_header() {
    let (ins, p1, p2, data) = parse_command(&[0x00, 0xE0, 0x11, 0x22]).expect("must parse");
    assert_eq!(ins, 0xE0);
    assert_eq!(p1, 0x11);
    assert_eq!(p2, 0x22);
    assert_eq!(data, &[] as &[u8], "no Lc byte → empty data");
}

// @internal
#[test]
fn parse_command_with_lc_extracts_data() {
    let cmd = [0x00, 0xE0, 0x00, 0x00, 0x03, 0xAA, 0xBB, 0xCC];
    let (ins, p1, p2, data) = parse_command(&cmd).expect("must parse");
    assert_eq!(ins, 0xE0);
    assert_eq!(p1, 0x00);
    assert_eq!(p2, 0x00);
    assert_eq!(data, &[0xAA, 0xBB, 0xCC]);
}

// @internal
#[test]
fn parse_command_returns_empty_data_when_lc_overruns_input() {
    // Lc claims 5 bytes but only 1 follows.
    let cmd = [0x00, 0xE0, 0x00, 0x00, 0x05, 0x01];
    let (_, _, _, data) = parse_command(&cmd).expect("must parse header");
    assert_eq!(
        data,
        &[] as &[u8],
        "truncated payload: data slice should be empty"
    );
}

// @internal
#[test]
fn parse_command_rejects_too_short_header() {
    assert!(parse_command(&[]).is_none());
    assert!(parse_command(&[0x00]).is_none());
    assert!(parse_command(&[0x00, 0xE0]).is_none());
    assert!(parse_command(&[0x00, 0xE0, 0x00]).is_none());
}

// @internal
#[test]
fn parse_command_with_only_lc_byte_returns_empty_data() {
    // 5 bytes total, Lc = 0 — valid empty-payload SELECT-style command.
    let (_, _, _, data) = parse_command(&[0x00, 0xA4, 0x04, 0x00, 0x00]).expect("must parse");
    assert_eq!(data, &[] as &[u8]);
}

// ============================================================
// is_select_vauchi
// ============================================================

// @internal
#[test]
fn is_select_vauchi_rejects_wrong_ins() {
    let mut cmd = build_select();
    cmd[1] = 0xA5;
    assert!(!is_select_vauchi(&cmd));
}

// @internal
#[test]
fn is_select_vauchi_rejects_wrong_p1() {
    let mut cmd = build_select();
    cmd[2] = 0x00;
    assert!(!is_select_vauchi(&cmd));
}

// @internal
#[test]
fn is_select_vauchi_rejects_wrong_aid() {
    let bogus = [0xF0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let mut cmd = vec![0x00, INS_SELECT, 0x04, 0x00, bogus.len() as u8];
    cmd.extend_from_slice(&bogus);
    assert!(!is_select_vauchi(&cmd));
}

// @internal
#[test]
fn is_select_vauchi_rejects_short_input() {
    assert!(!is_select_vauchi(&[]));
    assert!(!is_select_vauchi(&[0x00, INS_SELECT]));
}

// ============================================================
// is_exchange_data / is_card_exchange edge cases
// ============================================================

// @internal
#[test]
fn is_exchange_data_rejects_short_input() {
    assert!(!is_exchange_data(&[]));
    assert!(!is_exchange_data(&[0x00]));
}

// @internal
#[test]
fn is_exchange_data_rejects_other_ins() {
    assert!(!is_exchange_data(&[0x00, INS_CARD_EXCHANGE, 0x00, 0x00]));
    assert!(!is_exchange_data(&[0x00, INS_SELECT, 0x04, 0x00]));
}

// @internal
#[test]
fn is_card_exchange_rejects_short_input() {
    assert!(!is_card_exchange(&[]));
    assert!(!is_card_exchange(&[0x00]));
}

// @internal
#[test]
fn is_card_exchange_rejects_other_ins() {
    assert!(!is_card_exchange(&[0x00, INS_EXCHANGE_DATA, 0x00, 0x00]));
    assert!(!is_card_exchange(&[0x00, INS_SELECT, 0x04, 0x00]));
}

// ============================================================
// Property-based roundtrips (CC-04)
// ============================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Any payload up to 255 bytes survives build → parse_command → data.
    /// 255 is the single-byte Lc maximum.
    // @internal
    #[test]
    fn prop_exchange_data_roundtrip(payload in proptest::collection::vec(any::<u8>(), 0..=255)) {
        let cmd = build_exchange_data(&payload);
        let (ins, p1, p2, data) = parse_command(&cmd).expect("built APDU must parse");
        prop_assert_eq!(ins, INS_EXCHANGE_DATA);
        prop_assert_eq!(p1, 0x00);
        prop_assert_eq!(p2, 0x00);
        prop_assert_eq!(data, &payload[..]);
        prop_assert!(is_exchange_data(&cmd));
        prop_assert!(!is_card_exchange(&cmd));
        prop_assert!(!is_select_vauchi(&cmd));
    }

    /// Same property for `build_card_exchange`.
    // @internal
    #[test]
    fn prop_card_exchange_roundtrip(blob in proptest::collection::vec(any::<u8>(), 0..=255)) {
        let cmd = build_card_exchange(&blob);
        let (ins, p1, p2, data) = parse_command(&cmd).expect("built APDU must parse");
        prop_assert_eq!(ins, INS_CARD_EXCHANGE);
        prop_assert_eq!(p1, 0x00);
        prop_assert_eq!(p2, 0x00);
        prop_assert_eq!(data, &blob[..]);
        prop_assert!(is_card_exchange(&cmd));
        prop_assert!(!is_exchange_data(&cmd));
        prop_assert!(!is_select_vauchi(&cmd));
    }

    /// `parse_response` is the inverse of `data || sw` concatenation
    /// for any data of any length.
    // @internal
    #[test]
    fn prop_parse_response_inverse_of_concat(
        data in proptest::collection::vec(any::<u8>(), 0..512),
        sw1 in any::<u8>(),
        sw2 in any::<u8>(),
    ) {
        let mut buf = data.clone();
        buf.push(sw1);
        buf.push(sw2);
        let (out_data, out_sw) = parse_response(&buf).expect("at least 2 bytes");
        prop_assert_eq!(out_data, &data[..]);
        prop_assert_eq!(out_sw, [sw1, sw2]);
    }

    /// `parse_command` never panics on arbitrary bytes (CC-13 panic-freedom).
    /// The discriminating assertion is the absence of a panic. To stay
    /// compliant with the zero-assertion lint we also pin the structural
    /// invariant that whenever parsing succeeds, the returned data slice
    /// is a sub-slice of the input.
    // @internal
    #[test]
    fn prop_parse_command_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        if let Some((_, _, _, data)) = parse_command(&bytes) {
            prop_assert!(data.len() <= bytes.len(), "data slice exceeds input length");
        }
    }

    /// `is_select_vauchi` returns true iff the command parses as a SELECT
    /// for our exact AID — no other input should match.
    // @internal
    #[test]
    fn prop_is_select_vauchi_implies_correct_shape(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        if is_select_vauchi(&bytes) {
            let (ins, p1, _, data) = parse_command(&bytes).expect("must parse if recognized");
            prop_assert_eq!(ins, INS_SELECT);
            prop_assert_eq!(p1, 0x04);
            prop_assert_eq!(data, AID);
        }
    }
}
