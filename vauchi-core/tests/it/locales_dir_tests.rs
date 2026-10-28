// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Without a `locales` checkout every i18n test failed on its own content
//! assertion ("German FAQ must contain 'Telefon'"), 64 of them in a cold
//! checkout, and none named the cause (private#879). The lookup now stops
//! at the first use and says how to get the checkout.

use std::path::PathBuf;

use crate::common::checked_locales_dir;

fn checkout_with_english() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("en.json"), "{}").expect("write en.json");
    dir
}

// @internal
#[test]
#[should_panic(expected = "git clone git@gitlab.com:vauchi/locales.git")]
fn a_missing_sibling_checkout_names_the_clone_command() {
    let empty = tempfile::tempdir().expect("tempdir");

    checked_locales_dir(None, empty.path().to_path_buf());
}

// @internal
#[test]
#[should_panic(expected = "VAUCHI_LOCALES_DIR")]
fn a_locales_dir_variable_without_english_names_the_variable() {
    let empty = tempfile::tempdir().expect("tempdir");
    let sibling = checkout_with_english();

    checked_locales_dir(
        Some(empty.path().to_path_buf()),
        sibling.path().to_path_buf(),
    );
}

// @internal
#[test]
fn a_sibling_checkout_with_english_is_used() {
    let sibling = checkout_with_english();

    assert_eq!(
        checked_locales_dir(None, sibling.path().to_path_buf()),
        sibling.path().to_path_buf()
    );
}

// @internal
#[test]
fn the_variable_wins_over_the_sibling_checkout() {
    let from_env = checkout_with_english();
    let sibling = checkout_with_english();

    assert_eq!(
        checked_locales_dir(
            Some(from_env.path().to_path_buf()),
            sibling.path().to_path_buf()
        ),
        PathBuf::from(from_env.path())
    );
}
