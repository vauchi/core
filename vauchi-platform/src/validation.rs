// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UniFFI validation helpers (G3 of the frontend pure-renderer remediation).
//!
//! Frontends that previously hardcoded password/PIN/phone/email/URL rules
//! call these helpers instead of duplicating the logic. Core remains the
//! single source of truth for what counts as valid.
//!
//! Length constants exposed so frontends can render hint text and disable
//! buttons without baking the numbers into native code. For password
//! strength checking frontends use [`check_password_strength`] (legacy
//! surface, already exported from `lib.rs`).

use vauchi_core::contact_card::{is_valid_email, is_valid_phone, is_valid_relay_url};
use vauchi_core::identity::password::MIN_PASSWORD_LENGTH;

/// Minimum length accepted for a numeric PIN (e.g. duress PIN, app unlock PIN).
///
/// Stays in sync with the per-platform `MIN_PASSCODE_LENGTH` once the
/// frontends migrate to call this constant via UniFFI.
#[uniffi::export]
pub fn passcode_min_length() -> u32 {
    4
}

/// Maximum length accepted in passcode entry surfaces.
///
/// Bounded so heap allocations stay sane and to keep timing-attack windows
/// short. 64 bytes is comfortably above any realistic passphrase.
#[uniffi::export]
pub fn passcode_max_length() -> u32 {
    64
}

/// Minimum length for an alphanumeric app password.
///
/// Lower-bounds the password setup surfaces; passwords below this length are
/// rejected before zxcvbn even runs.
#[uniffi::export]
pub fn password_min_length() -> u32 {
    MIN_PASSWORD_LENGTH as u32
}

/// Check whether a string is a well-formed email address.
#[uniffi::export]
pub fn mobile_is_valid_email(value: String) -> bool {
    is_valid_email(&value)
}

/// Check whether a string is a well-formed phone number.
#[uniffi::export]
pub fn mobile_is_valid_phone(value: String) -> bool {
    is_valid_phone(&value)
}

/// Check whether a relay URL is well-formed and uses an accepted scheme.
#[uniffi::export]
pub fn mobile_is_valid_relay_url(url: String) -> bool {
    is_valid_relay_url(&url)
}
