// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use proptest::prelude::*;
use vauchi_core::text::normalize_text;

// @internal
#[test]
fn test_nfc_nfd_equivalence() {
    let nfc = "Jos\u{00E9}";
    let nfd = "Jose\u{0301}";
    assert_eq!(normalize_text(nfc), normalize_text(nfd));
}

// @internal
#[test]
fn test_combining_tilde() {
    let nfc = "\u{00F1}";
    let nfd = "n\u{0303}";
    assert_eq!(normalize_text(nfc), normalize_text(nfd));
}

// @internal
#[test]
fn test_already_nfc_passthrough() {
    assert_eq!(normalize_text("hello"), "hello");
    assert_eq!(normalize_text("Alice"), "Alice");
}

// @internal
#[test]
fn test_trim() {
    assert_eq!(normalize_text("  Alice  "), "Alice");
    assert_eq!(normalize_text("\tBob\n"), "Bob");
}

// @internal
#[test]
fn test_empty_string() {
    assert_eq!(normalize_text(""), "");
}

// @internal
#[test]
fn test_ascii_only_noop() {
    let hex = "4a6f7365";
    assert_eq!(normalize_text(hex), hex);
}

proptest! {
// @internal
    #[test]
    fn prop_normalization_idempotent(s in "\\PC{0,100}") {
        let once = normalize_text(&s);
        let twice = normalize_text(&once);
        prop_assert_eq!(&once, &twice, "normalization must be idempotent");
    }
}
