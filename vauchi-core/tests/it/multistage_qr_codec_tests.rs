// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::multistage::qr_codec::*;

// @internal
#[test]
fn test_parse_init_qr() {
    let _pubkey = [1u8; 32];
    let ephemeral = [2u8; 32];
    let commitment = [3u8; 32];
    let session_id = [4u8; 16];
    let qr = format_ini2_qr_with_relay(&session_id, &ephemeral, &commitment, "Alice", None, None);
    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Init {
            session_id: sid,
            ephemeral: eph,
            commitment_hash,
            display_name,
            relay_url,
            relay_noise_pubkey,
        } => {
            assert_eq!(sid, session_id);
            assert_eq!(eph, ephemeral);
            assert_eq!(commitment_hash, commitment);
            assert_eq!(display_name, "Alice");
            assert!(relay_url.is_none());
            assert!(relay_noise_pubkey.is_none());
        }
        _ => panic!("expected Init"),
    }
}

// @internal
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

// @internal
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

// @internal
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

// @internal
#[test]
fn test_parse_unknown_prefix() {
    let result = parse_qr("UNKNOWN:data");
    result.expect_err("expected error");
}

// @internal
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

// @internal
#[test]
fn test_init_qr_with_relay_url_but_no_noise_pubkey_is_rejected() {
    let session_id = [10u8; 16];
    let _pubkey = [11u8; 32];
    let ephemeral = [12u8; 32];
    let commitment = [13u8; 32];
    let relay_url = "https://relay.example.com";

    // Format encodes whatever is given — the fail-closed check is at parse time
    let qr = format_ini2_qr_with_relay(
        &session_id,
        &ephemeral,
        &commitment,
        "Bob",
        Some(relay_url),
        None,
    );

    // Parser enforces: relay URL without Noise pubkey → MissingRelayNoisePubkey
    let err = parse_qr(&qr).unwrap_err();
    assert!(
        format!("{err:?}").contains("MissingRelayNoisePubkey"),
        "relay URL without noise pubkey must be rejected, got: {err:?}"
    );
}

// @internal
#[test]
fn test_init_qr_with_relay_url_and_noise_pubkey() {
    let session_id = [14u8; 16];
    let _pubkey = [15u8; 32];
    let ephemeral = [16u8; 32];
    let commitment = [17u8; 32];
    let relay_url = "https://relay.example.com";
    let noise_pubkey = [18u8; 32];

    let qr = format_ini2_qr_with_relay(
        &session_id,
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

// @internal
#[test]
fn test_init_qr_without_relay_backward_compat() {
    // format_init_qr (no relay) should still parse correctly
    let session_id = [19u8; 16];
    let _pubkey = [20u8; 32];
    let ephemeral = [21u8; 32];
    let commitment = [22u8; 32];

    let qr = format_ini2_qr_with_relay(&session_id, &ephemeral, &commitment, "Dave", None, None);
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

// @internal
#[test]
fn test_init_qr_rejects_private_host_relay_url() {
    let session_id = [30u8; 16];
    let _pubkey = [31u8; 32];
    let ephemeral = [32u8; 32];
    let commitment = [33u8; 32];

    let qr = format_ini2_qr_with_relay(
        &session_id,
        &ephemeral,
        &commitment,
        "Evil",
        Some("https://127.0.0.1/evil"),
        None,
    );

    let result = parse_qr(&qr);
    assert!(result.is_err(), "private host relay URL should be rejected");
}

// @internal
#[test]
fn test_init_qr_rejects_insecure_scheme() {
    let session_id = [34u8; 16];
    let _pubkey = [35u8; 32];
    let ephemeral = [36u8; 32];
    let commitment = [37u8; 32];

    let qr = format_ini2_qr_with_relay(
        &session_id,
        &ephemeral,
        &commitment,
        "Evil",
        Some("http://relay.evil.com"),
        None,
    );

    let result = parse_qr(&qr);
    assert!(
        result.is_err(),
        "insecure scheme relay URL should be rejected"
    );
}

// @internal
#[test]
fn test_init_qr_truncated_before_flags() {
    let session_id = [38u8; 16];
    let _pubkey = [39u8; 32];
    let ephemeral = [40u8; 32];
    let commitment = [41u8; 32];

    let full_qr = format_ini2_qr_with_relay(&session_id, &ephemeral, &commitment, "X", None, None);

    // Truncate 2 chars (flags field) off the end
    let truncated = &full_qr[..full_qr.len() - 2];
    let result = parse_qr(truncated);
    assert!(result.is_err(), "truncated INIT QR should fail");
}

// @internal
#[test]
fn test_init_qr_with_only_noise_pubkey() {
    let session_id = [23u8; 16];
    let _pubkey = [24u8; 32];
    let ephemeral = [25u8; 32];
    let commitment = [26u8; 32];
    let noise_pubkey = [27u8; 32];

    let qr = format_ini2_qr_with_relay(
        &session_id,
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

// @internal
#[test]
fn test_relay_url_without_noise_pubkey_rejected_tofu() {
    // TOFU fail-closed: relay URL present but Noise pubkey missing → reject
    let _pubkey = [1u8; 32];
    let ephemeral = [2u8; 32];
    let commitment = [3u8; 32];
    let session_id = [4u8; 16];

    let qr = format_ini2_qr_with_relay(
        &session_id,
        &ephemeral,
        &commitment,
        "Test",
        Some("https://relay.example.com"),
        None,
    );
    let result = parse_qr(&qr);
    assert!(
        result.is_err(),
        "relay URL without Noise pubkey must be rejected (TOFU fail-closed)"
    );
}

// === SHAK stage (accel-envelope co-location) ===

// @internal
#[test]
fn test_parse_shake_qr_roundtrips_sealed_envelope() {
    let session_id = [42u8; 16];
    // Opaque AEAD-sealed bytes (nonce || ct+tag) from the slice-1 core.
    let sealed = vauchi_core::exchange::multistage::accel_envelope::seal_envelope(
        &[0x11; 32],
        &session_id,
        &[0.0, 1.0, 2.5, 5.0, 8.0],
    );
    let qr = format_shake_qr(&session_id, &sealed);
    let parsed = parse_qr(&qr).unwrap();
    match parsed {
        StageQr::Shake {
            session_id: sid,
            sealed_envelope,
        } => {
            assert_eq!(sid, session_id);
            assert_eq!(sealed_envelope, sealed);
        }
        _ => panic!("expected Shake"),
    }
}

// @internal
#[test]
fn test_shake_qr_crc_rejects_corrupted_payload() {
    let session_id = [7u8; 16];
    // Two distinct envelopes → distinct CRC and payload regions.
    let sealed_a = vauchi_core::exchange::multistage::accel_envelope::seal_envelope(
        &[0x11; 32],
        &session_id,
        &[1.0; 10],
    );
    let sealed_b = vauchi_core::exchange::multistage::accel_envelope::seal_envelope(
        &[0x11; 32],
        &session_id,
        &[2.0; 40],
    );
    let qr_a = format_shake_qr(&session_id, &sealed_a);
    let qr_b = format_shake_qr(&session_id, &sealed_b);

    // Layout: "SHAK"(4) + sid(24) + crc(3) + payload. Splice A's crc onto
    // B's payload → CRC describes A but covers B's bytes → mismatch.
    let header_len = 4 + 24 + 3;
    let frankenstein = format!("{}{}", &qr_a[..header_len], &qr_b[header_len..]);
    let err = parse_qr(&frankenstein).unwrap_err();
    assert!(
        format!("{err:?}").contains("CrcMismatch"),
        "corrupted SHAK payload must fail CRC, got: {err:?}"
    );
}

// @internal
#[test]
fn test_shake_qr_fits_single_dense_qr() {
    // 300 samples → 301-byte envelope + 28 AEAD overhead = 329 sealed bytes.
    let session_id = [3u8; 16];
    let samples: Vec<f32> = (0..300)
        .map(|i| (i as f32 / 100.0).sin().abs() * 3.0)
        .collect();
    let sealed = vauchi_core::exchange::multistage::accel_envelope::seal_envelope(
        &[0x11; 32],
        &session_id,
        &samples,
    );
    let qr = format_shake_qr(&session_id, &sealed);
    // base45 expands bytes ~1.5×; well under QR alphanumeric capacity (~4296).
    assert!(
        qr.len() < 1000,
        "SHAK QR unexpectedly large: {} chars",
        qr.len()
    );
    // Roundtrips intact.
    assert!(matches!(parse_qr(&qr).unwrap(), StageQr::Shake { .. }));
}
