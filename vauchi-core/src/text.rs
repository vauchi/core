// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Text normalization for cross-platform string consistency.
//!
//! All user-provided text (display names, field labels, field values) is
//! normalized to NFC (Canonical Decomposition + Canonical Composition) at
//! system boundaries. This ensures strings are byte-identical regardless
//! of input platform (macOS emits NFD, Linux/Windows emit NFC).

use unicode_normalization::UnicodeNormalization;

/// Normalize user-provided text to NFC, trimming leading/trailing whitespace.
///
/// Applied at every entry point where platform-dependent input enters core.
/// For strings already in NFC, this is a fast no-op scan.
pub fn normalize_text(s: &str) -> String {
    s.trim().nfc().collect()
}
