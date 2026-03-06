// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::{BleChunker, BleReassembler, BLE_CHUNK_OVERHEAD};

#[test]
fn test_chunker_single_chunk_small_payload() {
    let data = vec![0xAB; 10];
    let mtu_usable = 178;
    let chunker = BleChunker::new(&data, mtu_usable);

    assert_eq!(chunker.total_chunks(), 1);

    let chunk = chunker.chunk(0).expect("chunk 0 should exist");
    // Header: 2 bytes index + 2 bytes total
    assert_eq!(chunk.len(), BLE_CHUNK_OVERHEAD + data.len());

    // Verify header fields (little-endian)
    let index = u16::from_le_bytes([chunk[0], chunk[1]]);
    let total = u16::from_le_bytes([chunk[2], chunk[3]]);
    assert_eq!(index, 0);
    assert_eq!(total, 1);

    // Verify payload data
    assert_eq!(&chunk[BLE_CHUNK_OVERHEAD..], &data[..]);
}

#[test]
fn test_chunker_multiple_chunks() {
    let data = vec![0xCC; 500];
    let mtu_usable = 178;
    let max_data_per_chunk = mtu_usable - BLE_CHUNK_OVERHEAD; // 174
    let expected_chunks: u16 = ((500 + max_data_per_chunk - 1) / max_data_per_chunk) as u16; // ceil(500/174) = 3

    let chunker = BleChunker::new(&data, mtu_usable);
    assert_eq!(chunker.total_chunks(), expected_chunks);
    assert_eq!(expected_chunks, 3);

    // Every chunk should have the correct total_chunks in its header
    for i in 0..expected_chunks {
        let chunk = chunker.chunk(i).expect("chunk should exist");
        let total = u16::from_le_bytes([chunk[2], chunk[3]]);
        assert_eq!(total, expected_chunks);
    }
}

#[test]
fn test_roundtrip_chunking_reassembly() {
    let data: Vec<u8> = (0..2000).map(|i| (i % 256) as u8).collect();
    let mtu_usable = 178;

    let chunker = BleChunker::new(&data, mtu_usable);
    let total = chunker.total_chunks();
    assert!(total > 1, "2000 bytes should need multiple chunks");

    let mut reassembler = BleReassembler::new(total);
    for i in 0..total {
        let chunk = chunker.chunk(i).expect("chunk should exist");
        reassembler
            .insert_chunk(&chunk)
            .expect("insert should succeed");
    }

    assert!(reassembler.is_complete());
    assert_eq!(reassembler.received_count(), total);

    let assembled = reassembler.assemble().expect("should assemble");
    assert_eq!(assembled, data);
}

#[test]
fn test_reassembly_out_of_order() {
    let data: Vec<u8> = (0..500).map(|i| (i % 256) as u8).collect();
    let mtu_usable = 178;

    let chunker = BleChunker::new(&data, mtu_usable);
    let total = chunker.total_chunks();

    // Collect all chunks
    let chunks: Vec<Vec<u8>> = (0..total)
        .map(|i| chunker.chunk(i).expect("chunk should exist"))
        .collect();

    // Insert in reverse order
    let mut reassembler = BleReassembler::new(total);
    for chunk in chunks.iter().rev() {
        reassembler
            .insert_chunk(chunk)
            .expect("insert should succeed");
    }

    assert!(reassembler.is_complete());
    let assembled = reassembler.assemble().expect("should assemble");
    assert_eq!(assembled, data);
}

#[test]
fn test_reassembly_duplicate_chunk_is_idempotent() {
    let data = vec![0xDD; 400];
    let mtu_usable = 178;

    let chunker = BleChunker::new(&data, mtu_usable);
    let total = chunker.total_chunks();
    assert!(total > 1);

    let mut reassembler = BleReassembler::new(total);

    // Insert first chunk twice
    let chunk0 = chunker.chunk(0).expect("chunk 0 should exist");
    reassembler
        .insert_chunk(&chunk0)
        .expect("first insert should succeed");
    reassembler
        .insert_chunk(&chunk0)
        .expect("duplicate insert should succeed");

    // received_count should still be 1
    assert_eq!(reassembler.received_count(), 1);
    assert!(!reassembler.is_complete());
}

#[test]
fn test_reassembly_incomplete_returns_none() {
    let data = vec![0xEE; 600];
    let mtu_usable = 178;

    let chunker = BleChunker::new(&data, mtu_usable);
    let total = chunker.total_chunks();
    assert!(total > 1);

    let mut reassembler = BleReassembler::new(total);

    // Insert only the first chunk
    let chunk0 = chunker.chunk(0).expect("chunk 0 should exist");
    reassembler
        .insert_chunk(&chunk0)
        .expect("insert should succeed");

    assert!(!reassembler.is_complete());
    assert_eq!(reassembler.assemble(), None);
}

#[test]
fn test_chunk_index_out_of_range() {
    let data = vec![0xFF; 100];
    let mtu_usable = 178;

    let chunker = BleChunker::new(&data, mtu_usable);
    assert_eq!(chunker.total_chunks(), 1);

    // Index >= total should return None
    assert!(chunker.chunk(1).is_none());
    assert!(chunker.chunk(100).is_none());
    assert!(chunker.chunk(u16::MAX).is_none());
}
