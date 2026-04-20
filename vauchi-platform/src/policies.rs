// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Security / UX policy constants exposed to frontends (G5).
//!
//! Each policy lives in core so one value drives all platforms. Frontends
//! read the policy via UniFFI and execute it using their native platform
//! affordances (NSPasteboard on macOS/iOS, ClipboardManager on Android).
//!
//! Rationale for keeping policies in `vauchi-platform` rather than
//! `vauchi-core`: they are mobile-UX-specific and only frontends consume
//! them. CLI/TUI have no clipboard auto-clear affordance today. Promote to
//! `vauchi-core` if a Rust-native frontend grows the need.

/// Clipboard retention policy used when the user copies contact fields
/// (phone, email, address, etc.) to the OS pasteboard.
///
/// Frontends copy the value to their platform clipboard and then, after
/// `auto_clear_seconds`, re-read the clipboard and clear it **only if** the
/// current value still matches what was copied. This prevents Vauchi from
/// wiping clipboard content the user may have copied from another app
/// between the copy and the timer fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct MobileClipboardPolicy {
    /// Seconds to retain sensitive clipboard content before auto-clearing.
    ///
    /// Zero means "never auto-clear" — used for debug builds or when the
    /// user opts out. Production default is 30 seconds: long enough to
    /// paste once, short enough that clipboard managers don't harvest PII.
    pub auto_clear_seconds: u32,
}

const DEFAULT_CLIPBOARD_AUTO_CLEAR_SECONDS: u32 = 30;

/// Returns the current clipboard retention policy.
///
/// G5 of the frontend pure-renderer remediation: replaces
/// `ios/Vauchi/Views/ContactActions.swift:199-214` hardcoded 30-second
/// `Task.sleep(nanoseconds: 30_000_000_000)`.
#[uniffi::export]
pub fn mobile_clipboard_policy() -> MobileClipboardPolicy {
    MobileClipboardPolicy {
        auto_clear_seconds: DEFAULT_CLIPBOARD_AUTO_CLEAR_SECONDS,
    }
}

// ── Storage key lifecycle (G5 §3C) ──────────────────────────────────

const STORAGE_KEY_BYTE_LENGTH: u32 = 32;

/// Returns the exact byte length of the database storage key.
///
/// Used by frontends that cache the key in their platform keychain (iOS
/// Keychain, Android Keystore) so they can validate length before handing
/// the bytes to `PlatformAppEngine::new`. Replaces the
/// `private static let storageKeyLength = 32` literal in
/// `ios/Vauchi/Services/VauchiRepository.swift:456` (ADR-033 constant
/// that was being duplicated on every frontend).
#[uniffi::export]
pub fn mobile_storage_key_byte_length() -> u32 {
    STORAGE_KEY_BYTE_LENGTH
}

/// Generates a fresh random 32-byte storage key using core's audited CSPRNG.
///
/// Frontends call this on first launch and when a migration forces
/// regeneration (e.g. stored key has the wrong length). Replaces per-frontend
/// `SecRandomCopyBytes` / `SecureRandom.nextBytes` calls so the same RNG
/// (aws-lc-rs via RustCrypto) is used everywhere and key derivation stays
/// consistent across platforms.
#[uniffi::export]
pub fn mobile_generate_storage_key() -> Vec<u8> {
    vauchi_core::crypto::SymmetricKey::generate()
        .as_bytes()
        .to_vec()
}
