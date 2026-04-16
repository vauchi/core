// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::multistage::types::*;

// @internal
#[test]
fn test_protocol_state_default_is_idle() {
    let state = ProtocolState::default();
    assert!(matches!(state, ProtocolState::Idle));
}

// @internal
#[test]
fn test_chunk_bitmap_new_empty() {
    let bm = ChunkBitmap::new(10);
    assert_eq!(bm.total(), 10);
    assert_eq!(bm.received_count(), 0);
    assert!(!bm.is_complete());
}

// @internal
#[test]
fn test_chunk_bitmap_mark_and_check() {
    let mut bm = ChunkBitmap::new(4);
    bm.mark_received(0);
    bm.mark_received(2);
    assert!(bm.has(0));
    assert!(!bm.has(1));
    assert!(bm.has(2));
    assert!(!bm.has(3));
    assert_eq!(bm.received_count(), 2);
}

// @internal
#[test]
fn test_chunk_bitmap_complete() {
    let mut bm = ChunkBitmap::new(3);
    bm.mark_received(0);
    bm.mark_received(1);
    bm.mark_received(2);
    assert!(bm.is_complete());
}

// @internal
#[test]
fn test_chunk_bitmap_to_bytes_roundtrip() {
    let mut bm = ChunkBitmap::new(16);
    bm.mark_received(0);
    bm.mark_received(5);
    bm.mark_received(15);
    let bytes = bm.to_bytes();
    let bm2 = ChunkBitmap::from_bytes(&bytes, 16);
    assert!(bm2.has(0));
    assert!(bm2.has(5));
    assert!(bm2.has(15));
    assert!(!bm2.has(1));
}

// @internal
#[test]
fn test_qr_payload_has_error_correction() {
    let payload = QrPayload {
        data: "INIT:test".to_string(),
        error_correction: "M".to_string(),
        display_duration_ms: 0,
    };
    assert_eq!(payload.error_correction, "M");
}
