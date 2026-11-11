// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! REVEAL and FAIL roundtrips for `qr_codec`, and the retired frames.
//!
//! `multistage_qr_codec_tests.rs` covers INIT/DATA/VERIFY/CONFIRM.

use vauchi_core::exchange::multistage::qr_codec::{
    QrCodecError, StageQr, format_done_qr, format_fail_qr, format_final_qr,
    format_ini2_qr_with_relay, format_reveal_qr, parse_qr,
};

const SID: [u8; 16] = [0x11; 16];
const EPH: [u8; 32] = [0x33; 32];
const COMMITMENT: [u8; 32] = [0x44; 32];
const REVEAL_KEY: [u8; 32] = [0x55; 32];
const TAG: [u8; 32] = [0x66; 32];

// ============================================================
// REVEAL — the reveal key before this phone's checks passed
// ============================================================

// @internal
#[test]
fn reveal_qr_roundtrips_the_session_id_and_reveal_key() {
    let qr = format_reveal_qr(&SID, &REVEAL_KEY);

    assert_eq!(&qr[..4], "RVL4");
    match parse_qr(&qr).unwrap() {
        StageQr::Reveal {
            session_id,
            reveal_key,
        } => {
            assert_eq!(session_id, SID);
            assert_eq!(reveal_key, REVEAL_KEY);
        }
        other => panic!("expected Reveal, got {other:?}"),
    }
}

// @internal
#[test]
fn a_truncated_reveal_qr_is_an_error() {
    let qr = format_reveal_qr(&SID, &REVEAL_KEY);

    assert!(parse_qr(&qr[..qr.len() - 1]).is_err());
}

// @internal
#[test]
fn confirm_and_done_carry_their_own_prefixes() {
    assert_eq!(&format_final_qr(&SID, &REVEAL_KEY, &TAG)[..4], "CNF4");
    assert_eq!(&format_done_qr(&SID, &REVEAL_KEY, &TAG)[..4], "DON4");
}

// ============================================================
// Retired frames (vauchi/private#1217)
// ============================================================

// @internal
#[test]
fn the_retired_in3d_fin3_don3_and_bth3_frames_read_as_an_older_version() {
    let init = format_ini2_qr_with_relay(&SID, &EPH, &COMMITMENT, "Alice", None);
    for retired in ["IN3D", "FIN3", "DON3", "BTH3"] {
        let frame = format!("{retired}{}", &init[4..]);

        assert!(
            matches!(parse_qr(&frame), Err(QrCodecError::OldFormat)),
            "{retired} must be reported as an older version"
        );
    }
}

// ============================================================
// FAIL — abort broadcast
// ============================================================

// @internal
#[test]
fn fail_qr_roundtrips() {
    let qr = format_fail_qr(&SID);
    assert!(qr.starts_with("FAI3"));

    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Fail { session_id } => assert_eq!(session_id, SID),
        other => panic!("expected Fail, got {:?}", other),
    }
}

// @internal
#[test]
fn fail_qr_with_extra_trailing_chars_does_not_panic() {
    // Padded payload — implementation may accept or reject; either way
    // it must NOT panic. We pin the prefix-recognition invariant: even
    // on malformed input the parser still recognises the FAIL prefix
    // (i.e. it does not return UnknownPrefix), and a successful parse
    // produces a Fail variant rather than something else.
    let mut qr = format_fail_qr(&SID);
    qr.push_str("extra-data");
    let result = parse_qr(&qr);
    let acceptable = match &result {
        Ok(StageQr::Fail { .. }) => true,
        Ok(_) => false,
        Err(QrCodecError::UnknownPrefix) => false,
        Err(_) => true,
    };
    assert!(
        acceptable,
        "FAIL with trailing junk must either parse as Fail or fail with a non-UnknownPrefix error; got {:?}",
        result
    );
}

// ============================================================
// Cross-cutting: unknown prefix
// ============================================================

// @internal
#[test]
fn parse_qr_rejects_too_short_input() {
    assert!(parse_qr("").is_err());
    assert!(parse_qr("ABC").is_err());
}
