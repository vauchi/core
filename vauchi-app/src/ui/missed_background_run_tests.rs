// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The record sits outside the vault, so anything on disk under its name is
//! untrusted: only a whole, canonical number is a missed run (DC-01).

use proptest::prelude::*;

use super::*;

fn written(dir: &Path, bytes: &[u8]) {
    std::fs::write(dir.join(FILE_NAME), bytes).unwrap();
}

// @internal
#[test]
fn nothing_recorded_takes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(take_missed_background_run(dir.path()), None);
}

// @internal
#[test]
fn a_take_removes_the_record() {
    let dir = tempfile::tempdir().unwrap();
    record_missed_background_run(dir.path(), 42).unwrap();

    assert_eq!(take_missed_background_run(dir.path()), Some(42));
    assert_eq!(take_missed_background_run(dir.path()), None);
    assert!(!dir.path().join(FILE_NAME).exists());
}

// @internal
#[test]
fn malformed_records_are_refused_and_removed() {
    let cases: &[&[u8]] = &[
        b"",
        b" 42",
        b"42\n",
        b"+42",
        b"-1",
        b"042",
        b"4.2",
        b"0x2a",
        b"42\0",
        b"\xff\xfe",
        "４２".as_bytes(),
        b"18446744073709551616",
        &[b'9'; 4096],
    ];
    for bytes in cases {
        let dir = tempfile::tempdir().unwrap();
        written(dir.path(), bytes);

        assert_eq!(take_missed_background_run(dir.path()), None, "{bytes:?}");
        assert!(
            !dir.path().join(FILE_NAME).exists(),
            "{bytes:?} left behind"
        );
    }
}

// @internal
#[test]
fn a_directory_under_the_record_name_is_no_record() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(FILE_NAME)).unwrap();

    assert_eq!(take_missed_background_run(dir.path()), None);
}

proptest! {
    // @internal
    #[test]
    fn any_recorded_time_is_taken_back_unchanged(at in any::<u64>()) {
        let dir = tempfile::tempdir().unwrap();
        record_missed_background_run(dir.path(), at).unwrap();
        prop_assert_eq!(take_missed_background_run(dir.path()), Some(at));
    }

    // @internal
    #[test]
    fn arbitrary_bytes_are_a_record_only_when_canonical(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        let dir = tempfile::tempdir().unwrap();
        written(dir.path(), &bytes);
        let canonical = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| text.parse::<u64>().ok().filter(|n| n.to_string() == text));

        prop_assert_eq!(take_missed_background_run(dir.path()), canonical);
    }
}
