// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Common Test Utilities
//!
//! The shared arrange helpers live in `vauchi-core-testkit` (its crate docs
//! hold the idiom-to-helper table); they are re-exported here so every
//! `common::` path in this test crate stays as it was (vauchi/private#1230).
//! What depends on vauchi-app stays here until its tests move there.

#[allow(unused_imports)]
pub use vauchi_core_testkit::{
    card_update, device_sync, field_validation_helpers, fixtures, helpers, linked_peer, lost_phone,
    meeting, mock_relay, per_device, recovered_phone, signed_gateway, strategies, two_recipient,
    verifiers,
};

/// The sibling `locales` checkout. Prefers `VAUCHI_LOCALES_DIR` (exported by
/// CI and `just mutants-diff`) because cargo-mutants builds a temp copy where
/// the relative sibling does not exist.
pub fn locales_dir() -> std::path::PathBuf {
    checked_locales_dir(
        std::env::var_os("VAUCHI_LOCALES_DIR").map(std::path::PathBuf::from),
        std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR")
                .expect("CARGO_MANIFEST_DIR is set by cargo test and cargo nextest"),
        )
        .join("../../locales"),
    )
}

/// Stops at the first use when `en.json` is absent: otherwise every i18n
/// test fails on its own content assertion and none names the missing
/// checkout (private#879).
pub fn checked_locales_dir(
    from_env: Option<std::path::PathBuf>,
    sibling: std::path::PathBuf,
) -> std::path::PathBuf {
    let (dir, source) = match from_env {
        Some(dir) => (dir, "VAUCHI_LOCALES_DIR"),
        None => (sibling, "the sibling checkout beside core"),
    };
    assert!(
        dir.join("en.json").is_file(),
        "no locales checkout at {dir} ({source}): en.json is missing.\n  \
         Clone it: git clone git@gitlab.com:vauchi/locales.git {dir}\n  \
         or run `just setup` from the workspace root, \
         or set VAUCHI_LOCALES_DIR to a locales checkout",
        dir = dir.display(),
    );
    dir
}
