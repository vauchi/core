// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::multistage::chunker::Chunker;

#[test]
fn test_chunk_small_payload() {
    let data = vec![42u8; 100];
    let chunker = Chunker::new(&data, 800);
    assert_eq!(chunker.total_chunks(), 1);
    assert_eq!(chunker.chunk(0).unwrap(), &data[..]);
}

#[test]
fn test_chunk_exact_boundary() {
    let data = vec![0u8; 1600];
    let chunker = Chunker::new(&data, 800);
    assert_eq!(chunker.total_chunks(), 2);
    assert_eq!(chunker.chunk(0).unwrap().len(), 800);
    assert_eq!(chunker.chunk(1).unwrap().len(), 800);
}

#[test]
fn test_chunk_with_remainder() {
    let data = vec![0u8; 2100];
    let chunker = Chunker::new(&data, 800);
    assert_eq!(chunker.total_chunks(), 3);
    assert_eq!(chunker.chunk(0).unwrap().len(), 800);
    assert_eq!(chunker.chunk(1).unwrap().len(), 800);
    assert_eq!(chunker.chunk(2).unwrap().len(), 500);
}

#[test]
fn test_chunk_out_of_bounds() {
    let data = vec![0u8; 100];
    let chunker = Chunker::new(&data, 800);
    assert!(chunker.chunk(1).is_none());
}

#[test]
fn test_reassemble_from_chunks() {
    let original = (0..=255).cycle().take(2500).collect::<Vec<u8>>();
    let chunker = Chunker::new(&original, 800);

    let mut reassembled = Vec::new();
    for i in 0..chunker.total_chunks() {
        reassembled.extend_from_slice(chunker.chunk(i).unwrap());
    }
    assert_eq!(reassembled, original);
}

#[test]
fn test_reassembly_buffer() {
    use vauchi_core::exchange::multistage::chunker::ReassemblyBuffer;

    let original = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let chunker = Chunker::new(&original, 4);
    assert_eq!(chunker.total_chunks(), 3); // 4 + 4 + 2

    let mut buffer = ReassemblyBuffer::new(3);
    // Insert out of order
    buffer.insert(2, chunker.chunk(2).unwrap().to_vec());
    assert!(!buffer.is_complete());
    buffer.insert(0, chunker.chunk(0).unwrap().to_vec());
    assert!(!buffer.is_complete());
    buffer.insert(1, chunker.chunk(1).unwrap().to_vec());
    assert!(buffer.is_complete());

    let reassembled = buffer.assemble().unwrap();
    assert_eq!(reassembled, original);
}

#[test]
fn test_reassembly_buffer_duplicate_insert_ok() {
    use vauchi_core::exchange::multistage::chunker::ReassemblyBuffer;

    let mut buffer = ReassemblyBuffer::new(2);
    buffer.insert(0, vec![1, 2, 3]);
    buffer.insert(0, vec![1, 2, 3]); // duplicate — should not panic
    assert!(!buffer.is_complete());
}
