// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::multistage::qr_codec::*;

#[test]
fn test_parse_init_qr() {
    let pubkey = [1u8; 32];
    let ephemeral = [2u8; 32];
    let commitment = [3u8; 32];
    let session_id = [4u8; 16];
    let qr = format_init_qr(&session_id, &pubkey, &ephemeral, &commitment, "Alice");
    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Init {
            session_id: sid,
            pubkey: pk,
            ephemeral: eph,
            commitment_hash,
            display_name,
        } => {
            assert_eq!(sid, session_id);
            assert_eq!(pk, pubkey);
            assert_eq!(eph, ephemeral);
            assert_eq!(commitment_hash, commitment);
            assert_eq!(display_name, "Alice");
        }
        _ => panic!("expected Init"),
    }
}

#[test]
fn test_parse_data_qr() {
    let session_id = [5u8; 16];
    let ack_bitmap = vec![0b0000_0011];
    let payload = b"encrypted chunk data here";
    let qr = format_data_qr(&session_id, 2, 10, &ack_bitmap, payload);
    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Data {
            session_id: sid,
            chunk_idx,
            chunk_total,
            ack_bitmap: ack,
            crc: _,
            payload: p,
        } => {
            assert_eq!(sid, session_id);
            assert_eq!(chunk_idx, 2);
            assert_eq!(chunk_total, 10);
            assert_eq!(ack, vec![0b0000_0011]);
            assert_eq!(p, payload);
        }
        _ => panic!("expected Data"),
    }
}

#[test]
fn test_parse_verify_qr() {
    let session_id = [6u8; 16];
    let reveal_key = [7u8; 32];
    let qr = format_verify_qr(&session_id, &reveal_key);
    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Verify {
            session_id: sid,
            reveal_key: rk,
        } => {
            assert_eq!(sid, session_id);
            assert_eq!(rk, reveal_key);
        }
        _ => panic!("expected Verify"),
    }
}

#[test]
fn test_parse_confirm_qr() {
    let session_id = [8u8; 16];
    let payload_hash = [9u8; 32];
    let qr = format_confirm_qr(&session_id, &payload_hash);
    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Confirm {
            session_id: sid,
            payload_hash: ph,
        } => {
            assert_eq!(sid, session_id);
            assert_eq!(ph, payload_hash);
        }
        _ => panic!("expected Confirm"),
    }
}

#[test]
fn test_parse_unknown_prefix() {
    let result = parse_qr("UNKNOWN:data");
    assert!(result.is_err());
}

#[test]
fn test_data_qr_crc_integrity() {
    let session_id = [0u8; 16];
    let payload = b"test payload";
    let qr = format_data_qr(&session_id, 0, 1, &[0], payload);
    let parsed = parse_qr(&qr).unwrap();
    if let StageQr::Data {
        crc, payload: p, ..
    } = parsed
    {
        assert_eq!(crc, vauchi_core::exchange::multistage::crc16::compute(&p));
    } else {
        panic!("expected Data");
    }
}
