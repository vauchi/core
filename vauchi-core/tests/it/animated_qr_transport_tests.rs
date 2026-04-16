// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "testing")]

use vauchi_core::exchange::transport::animated_qr::{
    AnimatedQrConfig, AnimatedQrProgress, AnimatedQrSession,
};

fn default_config() -> AnimatedQrConfig {
    AnimatedQrConfig::default()
}

// --- Test 1: Encode payload into frames, verify frame_count > 1 for large payloads ---
#[test]
fn large_payload_produces_multiple_frames() {
    let payload = vec![0xAB; 2000]; // 2000 bytes > default 400 chunk_size
    let session = AnimatedQrSession::new_sender(payload, default_config());

    assert!(
        session.frame_count() > 1,
        "expected multiple frames for 2000-byte payload, got {}",
        session.frame_count()
    );
    // With 400-byte chunks, ceil(2000/400) = 5
    assert_eq!(session.frame_count(), 5);
}

// --- Test 2: next_frame cycles (wraps around after last frame) ---
#[test]
fn next_frame_cycles_around() {
    let payload = vec![0x42; 800]; // 2 frames
    let mut session = AnimatedQrSession::new_sender(payload, default_config());

    assert_eq!(session.frame_count(), 2);

    let frame0 = session.next_frame().expect("frame 0");
    let frame1 = session.next_frame().expect("frame 1");
    // Should wrap around
    let frame0_again = session.next_frame().expect("frame 0 again");

    assert_eq!(
        frame0, frame0_again,
        "next_frame should cycle back to first frame"
    );
    assert_ne!(frame0, frame1, "different frames should differ");
}

// --- Test 3: Receive frames out of order — reassemble correctly ---
#[test]
fn receive_out_of_order_reassembles_correctly() {
    let payload = vec![0xCD; 1200]; // 3 frames
    let mut sender = AnimatedQrSession::new_sender(payload.clone(), default_config());
    let mut receiver = AnimatedQrSession::new_receiver(default_config());

    assert_eq!(sender.frame_count(), 3);

    // Collect all frames
    let f0 = sender.next_frame().unwrap();
    let f1 = sender.next_frame().unwrap();
    let f2 = sender.next_frame().unwrap();

    // Process in reverse order
    let p2 = receiver.process_frame(&f2).expect("process f2");
    assert!(matches!(
        p2,
        AnimatedQrProgress::Partial {
            received: 1,
            total: 3
        }
    ));

    let p0 = receiver.process_frame(&f0).expect("process f0");
    assert!(matches!(
        p0,
        AnimatedQrProgress::Partial {
            received: 2,
            total: 3
        }
    ));

    let p1 = receiver.process_frame(&f1).expect("process f1");
    assert!(matches!(p1, AnimatedQrProgress::Complete));

    let reassembled = receiver.reassemble().expect("reassemble");
    assert_eq!(reassembled, payload);
}

// --- Test 4: Duplicate frames ignored ---
#[test]
fn duplicate_frames_ignored() {
    let payload = vec![0xEF; 800]; // 2 frames
    let mut sender = AnimatedQrSession::new_sender(payload.clone(), default_config());
    let mut receiver = AnimatedQrSession::new_receiver(default_config());

    let f0 = sender.next_frame().unwrap();
    let f1 = sender.next_frame().unwrap();

    let p0 = receiver.process_frame(&f0).expect("process f0");
    assert!(matches!(
        p0,
        AnimatedQrProgress::Partial {
            received: 1,
            total: 2
        }
    ));

    // Process f0 again — should still show 1 received
    let p0_dup = receiver.process_frame(&f0).expect("process f0 dup");
    assert!(
        matches!(
            p0_dup,
            AnimatedQrProgress::Partial {
                received: 1,
                total: 2
            }
        ),
        "duplicate frame should not increment received count"
    );

    let p1 = receiver.process_frame(&f1).expect("process f1");
    assert!(matches!(p1, AnimatedQrProgress::Complete));
}

// --- Test 5: Invalid frame rejected ---
#[test]
fn invalid_frame_rejected() {
    let mut receiver = AnimatedQrSession::new_receiver(default_config());

    // Completely malformed
    let result = receiver.process_frame("not-a-valid-frame");
    assert!(result.is_err(), "malformed frame should be rejected");

    // Wrong number of parts
    let result = receiver.process_frame("0/1");
    assert!(
        result.is_err(),
        "frame with too few parts should be rejected"
    );

    // Non-numeric index
    let result = receiver.process_frame("abc/2/deadbeef/AAAA");
    assert!(result.is_err(), "non-numeric index should be rejected");
}

// --- Test 6: Small payload → single frame ---
#[test]
fn small_payload_single_frame() {
    let payload = b"hello".to_vec();
    let session = AnimatedQrSession::new_sender(payload, default_config());

    assert_eq!(
        session.frame_count(),
        1,
        "small payload should produce exactly 1 frame"
    );
}

// --- Test 7: Frame format matches {index}/{total}/{crc32_hex}/{base64url} ---
#[test]
fn frame_format_matches_spec() {
    let payload = b"test data".to_vec();
    let mut session = AnimatedQrSession::new_sender(payload, default_config());

    let frame = session.next_frame().unwrap();
    let parts: Vec<&str> = frame.splitn(4, '/').collect();

    assert_eq!(parts.len(), 4, "frame should have 4 parts separated by /");

    // Part 0: index (0-based)
    let index: usize = parts[0].parse().expect("index should be numeric");
    assert_eq!(index, 0);

    // Part 1: total
    let total: usize = parts[1].parse().expect("total should be numeric");
    assert_eq!(total, 1);

    // Part 2: CRC32 hex (8 lowercase hex chars)
    assert_eq!(parts[2].len(), 8, "CRC32 hex should be 8 chars");
    assert!(
        parts[2].chars().all(|c| c.is_ascii_hexdigit()),
        "CRC32 should be hex characters"
    );

    // Part 3: base64url data (no padding)
    assert!(!parts[3].is_empty(), "base64url data should not be empty");
    assert!(
        !parts[3].contains('+') && !parts[3].contains('/') && !parts[3].contains('='),
        "should use base64url encoding without padding"
    );
}

// --- Test 8: Empty receiver cannot reassemble (missing chunks error) ---
#[test]
fn empty_receiver_cannot_reassemble() {
    let receiver = AnimatedQrSession::new_receiver(default_config());

    let result = receiver.reassemble();
    assert!(result.is_err(), "empty receiver should fail to reassemble");
}

// --- Test 9: CRC mismatch detected ---
#[test]
fn crc_mismatch_detected() {
    let payload = b"some data".to_vec();
    let mut sender = AnimatedQrSession::new_sender(payload, default_config());
    let mut receiver = AnimatedQrSession::new_receiver(default_config());

    let frame = sender.next_frame().unwrap();

    // Tamper with the CRC field
    let parts: Vec<&str> = frame.splitn(4, '/').collect();
    let tampered = format!("{}/{}/00000000/{}", parts[0], parts[1], parts[3]);

    let result = receiver.process_frame(&tampered);
    assert!(result.is_err(), "tampered CRC should be rejected");
}
