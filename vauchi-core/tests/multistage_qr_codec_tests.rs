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
    let qr = format_init_qr_with_relay(
        &session_id,
        &pubkey,
        &ephemeral,
        &commitment,
        "Alice",
        None,
        None,
    );
    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Init {
            session_id: sid,
            pubkey: pk,
            ephemeral: eph,
            commitment_hash,
            display_name,
            relay_url,
            relay_noise_pubkey,
        } => {
            assert_eq!(sid, session_id);
            assert_eq!(pk, pubkey);
            assert_eq!(eph, ephemeral);
            assert_eq!(commitment_hash, commitment);
            assert_eq!(display_name, "Alice");
            assert!(relay_url.is_none());
            assert!(relay_noise_pubkey.is_none());
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
    result.expect_err("expected error");
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

// === Relay URL in INIT QR ===

#[test]
fn test_init_qr_with_relay_url() {
    let session_id = [10u8; 16];
    let pubkey = [11u8; 32];
    let ephemeral = [12u8; 32];
    let commitment = [13u8; 32];
    let relay_url = "wss://relay.example.com";

    let qr = format_init_qr_with_relay(
        &session_id,
        &pubkey,
        &ephemeral,
        &commitment,
        "Bob",
        Some(relay_url),
        None,
    );

    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Init {
            session_id: sid,
            pubkey: pk,
            ephemeral: eph,
            commitment_hash,
            display_name,
            relay_url: url,
            relay_noise_pubkey: npk,
        } => {
            assert_eq!(sid, session_id);
            assert_eq!(pk, pubkey);
            assert_eq!(eph, ephemeral);
            assert_eq!(commitment_hash, commitment);
            assert_eq!(display_name, "Bob");
            assert_eq!(url.as_deref(), Some(relay_url));
            assert!(npk.is_none());
        }
        _ => panic!("expected Init"),
    }
}

#[test]
fn test_init_qr_with_relay_url_and_noise_pubkey() {
    let session_id = [14u8; 16];
    let pubkey = [15u8; 32];
    let ephemeral = [16u8; 32];
    let commitment = [17u8; 32];
    let relay_url = "wss://relay.example.com";
    let noise_pubkey = [18u8; 32];

    let qr = format_init_qr_with_relay(
        &session_id,
        &pubkey,
        &ephemeral,
        &commitment,
        "Carol",
        Some(relay_url),
        Some(&noise_pubkey),
    );

    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Init {
            relay_url: url,
            relay_noise_pubkey: npk,
            ..
        } => {
            assert_eq!(url.as_deref(), Some(relay_url));
            assert_eq!(npk.unwrap(), noise_pubkey);
        }
        _ => panic!("expected Init"),
    }
}

#[test]
fn test_init_qr_without_relay_backward_compat() {
    // format_init_qr (no relay) should still parse correctly
    let session_id = [19u8; 16];
    let pubkey = [20u8; 32];
    let ephemeral = [21u8; 32];
    let commitment = [22u8; 32];

    let qr = format_init_qr_with_relay(
        &session_id,
        &pubkey,
        &ephemeral,
        &commitment,
        "Dave",
        None,
        None,
    );
    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Init {
            display_name,
            relay_url,
            relay_noise_pubkey,
            ..
        } => {
            assert_eq!(display_name, "Dave");
            assert!(relay_url.is_none());
            assert!(relay_noise_pubkey.is_none());
        }
        _ => panic!("expected Init"),
    }
}

#[test]
fn test_init_qr_rejects_private_host_relay_url() {
    let session_id = [30u8; 16];
    let pubkey = [31u8; 32];
    let ephemeral = [32u8; 32];
    let commitment = [33u8; 32];

    let qr = format_init_qr_with_relay(
        &session_id,
        &pubkey,
        &ephemeral,
        &commitment,
        "Evil",
        Some("wss://127.0.0.1/evil"),
        None,
    );

    // Should fail SSRF validation during parse
    let result = parse_qr(&qr);
    assert!(result.is_err(), "private host relay URL should be rejected");
}

#[test]
fn test_init_qr_rejects_insecure_scheme() {
    let session_id = [34u8; 16];
    let pubkey = [35u8; 32];
    let ephemeral = [36u8; 32];
    let commitment = [37u8; 32];

    let qr = format_init_qr_with_relay(
        &session_id,
        &pubkey,
        &ephemeral,
        &commitment,
        "Evil",
        Some("ws://relay.evil.com"),
        None,
    );

    let result = parse_qr(&qr);
    assert!(
        result.is_err(),
        "insecure scheme relay URL should be rejected"
    );
}

#[test]
fn test_init_qr_truncated_before_flags() {
    // Build a valid prefix but truncate before flags byte
    let session_id = [38u8; 16];
    let pubkey = [39u8; 32];
    let ephemeral = [40u8; 32];
    let commitment = [41u8; 32];

    let full_qr = format_init_qr_with_relay(
        &session_id,
        &pubkey,
        &ephemeral,
        &commitment,
        "X",
        None,
        None,
    );

    // Truncate 2 chars (flags field) off the end
    let truncated = &full_qr[..full_qr.len() - 2];
    let result = parse_qr(truncated);
    assert!(result.is_err(), "truncated INIT QR should fail");
}

#[test]
fn test_init_qr_with_only_noise_pubkey() {
    let session_id = [23u8; 16];
    let pubkey = [24u8; 32];
    let ephemeral = [25u8; 32];
    let commitment = [26u8; 32];
    let noise_pubkey = [27u8; 32];

    let qr = format_init_qr_with_relay(
        &session_id,
        &pubkey,
        &ephemeral,
        &commitment,
        "Eve",
        None,
        Some(&noise_pubkey),
    );

    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Init {
            relay_url,
            relay_noise_pubkey,
            ..
        } => {
            assert!(relay_url.is_none());
            assert_eq!(relay_noise_pubkey.unwrap(), noise_pubkey);
        }
        _ => panic!("expected Init"),
    }
}

#[test]
fn test_relay_url_without_noise_pubkey_rejected_tofu() {
    // TOFU fail-closed: relay URL present but Noise pubkey missing → reject
    let pubkey = [1u8; 32];
    let ephemeral = [2u8; 32];
    let commitment = [3u8; 32];
    let session_id = [4u8; 16];

    let qr = format_init_qr_with_relay(
        &session_id,
        &pubkey,
        &ephemeral,
        &commitment,
        "Test",
        Some("wss://relay.example.com"),
        None,
    );
    let result = parse_qr(&qr);
    assert!(
        result.is_err(),
        "relay URL without Noise pubkey must be rejected (TOFU fail-closed)"
    );
}
