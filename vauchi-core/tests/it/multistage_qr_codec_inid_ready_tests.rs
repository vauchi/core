// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! INID, READY, COMBO, and FAIL roundtrips for `qr_codec`.
//!
//! `multistage_qr_codec_tests.rs` covers INIT/DATA/VERIFY/CONFIRM but
//! not these four variants. They were 0% covered before this file.

use vauchi_core::exchange::multistage::qr_codec::{
    StageQr, format_combo_qr, format_fail_qr, format_inid_qr, format_ready_qr, parse_qr,
};

const SID: [u8; 16] = [0x11; 16];
const PK: [u8; 32] = [0x22; 32];
const EPH: [u8; 32] = [0x33; 32];
const COMMITMENT: [u8; 32] = [0x44; 32];
const REVEAL_KEY: [u8; 32] = [0x55; 32];
const PAYLOAD_HASH: [u8; 32] = [0x66; 32];
const ACK_HASH: [u8; 32] = [0x77; 32];

// ============================================================
// INID — INIT-with-embedded-data, single-chunk fast path
// ============================================================

// @internal
#[test]
fn inid_qr_roundtrips_without_relay_fields() {
    let ciphertext = vec![0xAB; 64];
    let qr = format_inid_qr(
        &SID,
        &PK,
        &EPH,
        &COMMITMENT,
        "Alice",
        None,
        None,
        &ciphertext,
    );

    assert!(
        qr.starts_with("INID"),
        "QR must carry INID prefix, got: {}",
        &qr[..4]
    );

    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Inid {
            session_id,
            pubkey,
            ephemeral,
            commitment_hash,
            display_name,
            relay_url,
            relay_noise_pubkey,
            ciphertext: parsed_ct,
        } => {
            assert_eq!(session_id, SID);
            assert_eq!(pubkey, PK);
            assert_eq!(ephemeral, EPH);
            assert_eq!(commitment_hash, COMMITMENT);
            assert_eq!(display_name, "Alice");
            assert_eq!(relay_url, None);
            assert_eq!(relay_noise_pubkey, None);
            assert_eq!(parsed_ct, ciphertext);
        }
        other => panic!("expected Inid, got {:?}", other),
    }
}

// @internal
#[test]
fn inid_qr_roundtrips_with_relay_url_and_noise_pubkey() {
    let ciphertext = vec![0xCD; 32];
    let relay_url = "https://relay.example.com";
    let relay_npk = [0x99u8; 32];
    let qr = format_inid_qr(
        &SID,
        &PK,
        &EPH,
        &COMMITMENT,
        "Bob",
        Some(relay_url),
        Some(&relay_npk),
        &ciphertext,
    );

    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Inid {
            display_name,
            relay_url: parsed_url,
            relay_noise_pubkey: parsed_npk,
            ciphertext: parsed_ct,
            ..
        } => {
            assert_eq!(display_name, "Bob");
            assert_eq!(parsed_url.as_deref(), Some(relay_url));
            assert_eq!(parsed_npk, Some(relay_npk));
            assert_eq!(parsed_ct, ciphertext);
        }
        other => panic!("expected Inid, got {:?}", other),
    }
}

// @internal
#[test]
fn inid_qr_with_relay_url_but_no_noise_pubkey_is_rejected() {
    let qr = format_inid_qr(
        &SID,
        &PK,
        &EPH,
        &COMMITMENT,
        "Alice",
        Some("https://relay.example.com"),
        None,
        &[0xAA; 16],
    );
    let result = parse_qr(&qr);
    assert!(
        result.is_err(),
        "INID with relay URL but no Noise pubkey must be rejected (TOFU MITM defense)"
    );
}

// @internal
#[test]
fn inid_qr_handles_empty_ciphertext() {
    let qr = format_inid_qr(&SID, &PK, &EPH, &COMMITMENT, "Alice", None, None, &[]);
    let parsed = parse_qr(&qr).unwrap();
    if let StageQr::Inid { ciphertext, .. } = parsed {
        assert!(ciphertext.is_empty());
    } else {
        panic!("expected Inid");
    }
}

// ============================================================
// READY — backward-compat handshake
// ============================================================

// @internal
#[test]
fn ready_qr_roundtrips() {
    let qr = format_ready_qr(&SID, &ACK_HASH);
    assert!(qr.starts_with("RDYY"));

    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Ready {
            session_id,
            ack_hash,
        } => {
            assert_eq!(session_id, SID);
            assert_eq!(ack_hash, ACK_HASH);
        }
        other => panic!("expected Ready, got {:?}", other),
    }
}

// @internal
#[test]
fn ready_qr_truncated_body_is_rejected() {
    let mut qr = format_ready_qr(&SID, &ACK_HASH);
    qr.truncate(qr.len() - 5);
    assert!(parse_qr(&qr).is_err(), "truncated RDYY must error");
}

// ============================================================
// CMBO — compound VRFY+CONF+RDYY
// ============================================================

// @internal
#[test]
fn combo_qr_roundtrips_carrying_all_three_components() {
    let qr = format_combo_qr(&SID, &REVEAL_KEY, &PAYLOAD_HASH, &ACK_HASH);
    assert!(qr.starts_with("CMBO"));
    assert_eq!(qr.len(), 4 + 24 + 48 * 3, "CMBO total layout per ADR");

    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Combo {
            session_id,
            reveal_key,
            payload_hash,
            ack_hash,
        } => {
            assert_eq!(session_id, SID);
            assert_eq!(reveal_key, REVEAL_KEY);
            assert_eq!(payload_hash, PAYLOAD_HASH);
            assert_eq!(ack_hash, ACK_HASH);
        }
        other => panic!("expected Combo, got {:?}", other),
    }
}

// ============================================================
// FAIL — abort broadcast
// ============================================================

// @internal
#[test]
fn fail_qr_roundtrips() {
    let qr = format_fail_qr(&SID);
    assert!(qr.starts_with("FAIL"));

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
    use vauchi_core::exchange::multistage::qr_codec::QrCodecError;

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
