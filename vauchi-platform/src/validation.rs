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
use vauchi_core::recovery::{RECOVERY_CLAIM_MIN_INPUT_LEN, RECOVERY_PUBLIC_KEY_HEX_LEN};

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

/// Hex-encoded length of an Ed25519 identity public key (32 bytes ×
/// 2 hex characters = 64 characters).
///
/// Frontends (iOS `RecoveryView`, Android `RecoveryScreen`) gate the
/// "Create Claim" button on `oldPublicKey.length >= recovery_public_key_hex_length()`
/// instead of hardcoding `64`. Stays in sync with core's own usage in
/// the recovery flow.
#[uniffi::export]
pub fn recovery_public_key_hex_length() -> u32 {
    RECOVERY_PUBLIC_KEY_HEX_LEN as u32
}

/// Minimum length (in characters) of a recovery claim input string
/// before the "Verify Claim" button is enabled.
///
/// Heuristic — the actual claim parse happens in the `AppEngine`
/// intercept. Frontends gate the affordance on
/// `claim.length >= recovery_claim_min_input_length()` instead of
/// hardcoding `20`. Mirrors core's own usage in `recovery_help.rs`.
#[uniffi::export]
pub fn recovery_claim_min_input_length() -> u32 {
    RECOVERY_CLAIM_MIN_INPUT_LEN as u32
}

/// Check whether a string is a well-formed PEM-encoded X.509 certificate.
///
/// Accepts the trimmed input if it begins with
/// `-----BEGIN CERTIFICATE-----` and ends with
/// `-----END CERTIFICATE-----`. Other PEM labels (`PRIVATE KEY`,
/// `RSA PRIVATE KEY`, …) are rejected so the surface that consumes
/// the result can render a "this is not a certificate" hint.
///
/// This is the surface frontends use for certificate-pinning UI input
/// validation (replaces the per-frontend prefix/suffix checks). The
/// real cryptographic validation happens in the TLS layer when
/// [`crate::VauchiPlatform::set_pinned_certificate`] is consumed by
/// the rustls verifier; this helper exists to give users immediate
/// inline feedback while typing.
#[uniffi::export]
pub fn mobile_is_valid_pem_certificate(value: String) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("-----BEGIN CERTIFICATE-----")
        && trimmed.ends_with("-----END CERTIFICATE-----")
}
