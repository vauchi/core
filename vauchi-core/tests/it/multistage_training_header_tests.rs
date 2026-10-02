// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The link-training header every multi-stage frame carries.

use proptest::prelude::*;
use vauchi_core::exchange::multistage::qr_codec::*;
use vauchi_core::exchange::multistage::training_header::*;

fn reads(layout: u8, count: u8) -> LayoutReads {
    LayoutReads { layout, count }
}

fn busy_header() -> TrainingHeader {
    TrainingHeader::new(11, &[reads(3, 44), reads(0, 7), reads(13, 1)], 44)
        .expect("header within range")
}

/// One frame of every type, with the stage it must parse back to.
fn one_frame_of_every_type() -> Vec<(&'static str, String, StageQr)> {
    let session_id = [7u8; 16];
    let key = [1u8; 32];
    let hash = [2u8; 32];
    let other = [3u8; 32];
    vec![
        (
            "opening",
            format_ini2_qr_with_relay(&session_id, &key, &hash, "Alice", None),
            StageQr::Init {
                session_id,
                ephemeral: key,
                commitment_hash: hash,
                display_name: "Alice".into(),
                relay_url: None,
            },
        ),
        (
            "opening with data",
            format_in2d_qr(&session_id, &key, &hash, "Alice", None, &[9, 8, 7]),
            StageQr::Inid {
                session_id,
                ephemeral: key,
                commitment_hash: hash,
                display_name: "Alice".into(),
                relay_url: None,
                ciphertext: vec![9, 8, 7],
            },
        ),
        (
            "data",
            format_data_qr(&session_id, 2, 5, &[0b101], &[4, 5, 6, 7]),
            StageQr::Data {
                session_id,
                chunk_idx: 2,
                chunk_total: 5,
                ack_bitmap: vec![0b101],
                crc: vauchi_core::exchange::multistage::crc16::compute(&[4, 5, 6, 7]),
                payload: vec![4, 5, 6, 7],
            },
        ),
        (
            "final",
            format_final_qr(&session_id, &key, &other),
            StageQr::Final {
                session_id,
                reveal_key: key,
                tag: other,
            },
        ),
        (
            "fail",
            format_fail_qr(&session_id),
            StageQr::Fail { session_id },
        ),
        (
            "shake",
            format_shake_qr(&session_id, &[5; 40]),
            StageQr::Shake {
                session_id,
                sealed_envelope: vec![5; 40],
            },
        ),
    ]
}

// @internal
#[test]
fn every_frame_type_round_trips_with_its_training_header() {
    let header = busy_header();

    for (name, frame, stage) in one_frame_of_every_type() {
        let sent = with_header(&frame, &header).expect("a formatted frame takes a header");

        let parsed = parse_frame(&sent).unwrap_or_else(|e| panic!("{name} frame: {e}"));

        assert_eq!(parsed.header, header, "{name} frame");
        assert_eq!(parsed.stage, stage, "{name} frame");
    }
}

// @internal
#[test]
fn a_frame_carries_an_empty_header_until_one_is_set() {
    for (name, frame, _) in one_frame_of_every_type() {
        let parsed = parse_frame(&frame).unwrap_or_else(|e| panic!("{name} frame: {e}"));

        assert_eq!(parsed.header, TrainingHeader::default(), "{name} frame");
        assert_eq!(parsed.header.layout(), 0, "{name} frame");
        assert_eq!(parsed.header.echo().count(), 0, "{name} frame");
        assert_eq!(parsed.header.total_reads(), 0, "{name} frame");
    }
}

// @internal
#[test]
fn the_header_adds_eight_characters_to_a_frame() {
    assert_eq!(HEADER_LEN, 8);
    assert_eq!(busy_header().encode().len(), 8);
    // prefix 4 + header 8 + session id 24
    assert_eq!(format_fail_qr(&[0u8; 16]).len(), 36);
}

// @internal
#[test]
fn the_opening_frame_with_a_header_stays_in_qr_version_6() {
    // 18 bytes is the longest name that still fits; the session shortens a
    // longer one before it formats the frame.
    let name = "A".repeat(18);
    let frame = format_ini2_qr_with_relay(&[7u8; 16], &[1u8; 32], &[2u8; 32], &name, None);
    let sent = with_header(&frame, &busy_header()).expect("header fits");

    assert_eq!(sent.len(), 154);
    let code = qrcode::QrCode::with_error_correction_level(&sent, qrcode::EcLevel::M)
        .expect("frame encodes");
    assert_eq!(code.width(), 41);
}

// @internal
#[test]
fn a_frame_in_the_previous_format_is_told_apart_from_a_foreign_code() {
    let previous_format = [
        "INI2", "IN2D", "DATA", "VRFY", "CONF", "RDYY", "FAIL", "SHAK", "CMBO",
    ];
    for prefix in previous_format {
        let frame = format!("{prefix}{}", "0".repeat(120));
        assert!(
            matches!(parse_frame(&frame), Err(QrCodecError::OldFormat)),
            "{prefix}"
        );
    }

    for foreign in ["https://example.org/a-long-enough-path", "WIFI:S:net;;", ""] {
        assert!(
            matches!(parse_frame(foreign), Err(QrCodecError::UnknownPrefix)),
            "{foreign:?}"
        );
    }
}

// @internal
#[test]
fn a_header_outside_its_ranges_is_refused_at_construction() {
    let refused = [
        ("own layout past the set", TrainingHeader::new(14, &[], 0)),
        (
            "echoed layout past the set",
            TrainingHeader::new(0, &[reads(14, 1)], 0),
        ),
        (
            "echoed count of zero",
            TrainingHeader::new(0, &[reads(1, 0)], 0),
        ),
        (
            "echoed count past one digit",
            TrainingHeader::new(0, &[reads(1, 45)], 0),
        ),
        ("total past one digit", TrainingHeader::new(0, &[], 45)),
        (
            "a layout echoed twice",
            TrainingHeader::new(0, &[reads(2, 3), reads(2, 4)], 0),
        ),
        (
            "more echoes than slots",
            TrainingHeader::new(0, &[reads(1, 1), reads(2, 1), reads(3, 1), reads(4, 1)], 0),
        ),
    ];

    for (case, result) in refused {
        assert!(matches!(result, Err(QrCodecError::InvalidHeader)), "{case}");
    }
}

// @internal
#[test]
fn a_malformed_header_from_a_camera_is_refused() {
    let session = "0".repeat(24);
    let malformed = [
        ("own layout out of range", "E:0:0:00"),
        ("echoed layout out of range", "0E1:0:00"),
        ("unused slot with a count", "0:5:0:00"),
        ("used slot after an unused one", "0:011:00"),
        ("echoed count of zero", "010:0:00"),
        ("a layout echoed twice", "01111:00"),
        ("a character outside base45", "0:0:0:0a"),
        ("lower-case digits", "0:0:0:0z"),
        ("a multi-byte character", "0:0:0:é"),
    ];

    for (case, header) in malformed {
        let frame = format!("FAI3{header}{session}");
        assert!(
            matches!(parse_frame(&frame), Err(QrCodecError::InvalidHeader)),
            "{case}: {frame}"
        );
    }
}

// @internal
#[test]
fn a_frame_cut_inside_its_header_is_too_short() {
    for cut in ["FAI3", "FAI30:0", "FAI30:0:0:0"] {
        assert!(
            matches!(parse_frame(cut), Err(QrCodecError::TooShort)),
            "{cut}"
        );
    }
}

// @internal
#[test]
fn a_header_cannot_be_set_on_something_that_is_not_a_frame() {
    let header = busy_header();

    assert!(matches!(
        with_header("https://example.org", &header),
        Err(QrCodecError::UnknownPrefix)
    ));
    assert!(matches!(
        with_header("DATA0000", &header),
        Err(QrCodecError::OldFormat)
    ));
}

fn any_header() -> impl Strategy<Value = TrainingHeader> {
    (
        0..LAYOUT_COUNT,
        proptest::sample::subsequence((0..LAYOUT_COUNT).collect::<Vec<u8>>(), 0..=ECHO_SLOTS),
        proptest::collection::vec(1..=MAX_READ_COUNT, ECHO_SLOTS),
        0..=MAX_READ_COUNT,
    )
        .prop_map(|(layout, echoed, counts, total)| {
            let echo: Vec<LayoutReads> = echoed
                .into_iter()
                .zip(counts)
                .map(|(layout, count)| LayoutReads { layout, count })
                .collect();
            TrainingHeader::new(layout, &echo, total).expect("strategy stays within range")
        })
}

proptest! {
    // @internal
    #[test]
    fn any_header_round_trips_through_its_eight_digits(header in any_header()) {
        let encoded = header.encode();

        prop_assert_eq!(encoded.len(), HEADER_LEN);
        prop_assert_eq!(TrainingHeader::parse(&encoded).unwrap(), header);
    }

    // @internal
    #[test]
    fn any_header_round_trips_on_a_data_frame(
        header in any_header(),
        payload in proptest::collection::vec(any::<u8>(), 0..90),
        idx in 0u16..50,
    ) {
        let frame = format_data_qr(&[9u8; 16], idx, 50, &[0xFF], &payload);

        let parsed = parse_frame(&with_header(&frame, &header).unwrap()).unwrap();

        prop_assert_eq!(parsed.header, header);
        prop_assert!(
            matches!(parsed.stage, StageQr::Data { chunk_idx, payload: p, .. } if chunk_idx == idx && p == payload),
            "data frame content changed"
        );
    }

    // @internal
    #[test]
    fn no_eight_characters_make_the_parser_panic(text in "\\PC{0,12}") {
        // Whatever a camera reads after a valid prefix is either a header or
        // an error.
        let _ = parse_frame(&format!("FAI3{text}"));
    }

    // @internal
    #[test]
    fn a_parsed_header_always_encodes_back_to_what_was_read(digits in "[0-9A-Z $%*+./:-]{8}") {
        if let Ok(header) = TrainingHeader::parse(&digits) {
            prop_assert_eq!(header.encode(), digits);
        }
    }
}
