// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::{reassemble_chain, split_into_chain, MAX_APDU_DATA};

#[test]
fn test_small_payload_single_chunk() {
    let data = vec![0xAA; 100];
    let chunks = split_into_chain(0xE2, &data);
    assert_eq!(chunks.len(), 1, "100 bytes should fit in one APDU");
    assert_eq!(
        chunks[0][0], 0x00,
        "Single chunk CLA should be 0x00 (final)"
    );
    assert_eq!(chunks[0][1], 0xE2, "INS should be preserved");
}

#[test]
fn test_exact_boundary_single_chunk() {
    let data = vec![0xBB; MAX_APDU_DATA];
    let chunks = split_into_chain(0xE2, &data);
    assert_eq!(
        chunks.len(),
        1,
        "Exactly MAX_APDU_DATA bytes should be one chunk"
    );
}

#[test]
fn test_large_payload_multiple_chunks() {
    let data = vec![0xCC; MAX_APDU_DATA + 1];
    let chunks = split_into_chain(0xE2, &data);
    assert_eq!(chunks.len(), 2);
    assert_eq!(
        chunks[0][0], 0x10,
        "Non-final chunk CLA should have chaining bit set"
    );
    assert_eq!(chunks[1][0], 0x00, "Final chunk CLA should be 0x00");
}

#[test]
fn test_three_chunks() {
    let data = vec![0xDD; MAX_APDU_DATA * 2 + 50];
    let chunks = split_into_chain(0xE2, &data);
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0][0], 0x10);
    assert_eq!(chunks[1][0], 0x10);
    assert_eq!(chunks[2][0], 0x00);
}

#[test]
fn test_roundtrip_small() {
    let original = vec![0x42; 100];
    let chunks = split_into_chain(0xE2, &original);
    let reassembled = reassemble_chain(&chunks).expect("reassembly should succeed");
    assert_eq!(original, reassembled);
}

#[test]
fn test_roundtrip_large() {
    let original = vec![0x99; 600];
    let chunks = split_into_chain(0xE2, &original);
    assert_eq!(chunks.len(), 3); // 255 + 255 + 90
    let reassembled = reassemble_chain(&chunks).expect("reassembly should succeed");
    assert_eq!(original, reassembled);
}

#[test]
fn test_roundtrip_exact_boundary() {
    let original = vec![0xFF; MAX_APDU_DATA * 3];
    let chunks = split_into_chain(0xE2, &original);
    assert_eq!(chunks.len(), 3);
    let reassembled = reassemble_chain(&chunks).expect("reassembly should succeed");
    assert_eq!(original, reassembled);
}

#[test]
fn test_empty_payload() {
    let chunks = split_into_chain(0xE2, &[]);
    assert_eq!(chunks.len(), 1);
    let reassembled = reassemble_chain(&chunks).expect("reassembly should succeed");
    assert!(reassembled.is_empty());
}

#[test]
fn test_reassemble_empty_chain_fails() {
    let result = reassemble_chain(&[]);
    assert!(result.is_err());
}

#[test]
fn test_reassemble_missing_final_fails() {
    // A chain where the only command has chaining bit set — no final marker
    let cmd = vec![0x10, 0xE2, 0x00, 0x00, 0x01, 0xAA];
    let result = reassemble_chain(&[cmd]);
    assert!(result.is_err());
}

#[test]
fn test_reassemble_truncated_command_fails() {
    let short = vec![0x00, 0xE2]; // Too short — no Lc
    let result = reassemble_chain(&[short]);
    assert!(result.is_err());
}

#[test]
fn test_ins_byte_preserved() {
    let data = vec![0xAA; 10];
    let chunks = split_into_chain(0xE0, &data);
    assert_eq!(chunks[0][1], 0xE0, "INS byte must be preserved");
}
