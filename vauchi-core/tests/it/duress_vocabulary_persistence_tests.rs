// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a duress session makes — groups, tags, places — must still be
//! there after a lock and a second duress unlock, as the duress copy of
//! the own card is (vauchi/private#813, after #811): a group that vanished
//! after a lock would give the mode away.

use vauchi_core::Vauchi;
use vauchi_core::api::AuthMode;

use crate::common::helpers::create_vauchi_with_identity;

const APP_PIN: &str = "my-pin-1234";
const DURESS_PIN: &str = "654321";

fn alice_under_duress() -> Vauchi {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password(APP_PIN).unwrap();
    wb.setup_duress_password(DURESS_PIN).unwrap();
    assert_eq!(wb.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);
    wb
}

fn names<T>(items: Vec<T>, name: impl Fn(&T) -> String) -> Vec<String> {
    items.iter().map(name).collect()
}

// @scenario: duress_mode :: Changes made in duress mode look like they worked
#[test]
fn a_group_a_tag_and_a_place_made_under_duress_survive_a_second_duress_unlock() {
    let mut wb = alice_under_duress();
    wb.create_group("Work").unwrap();
    wb.create_tag("books").unwrap();
    wb.create_named_place("Bern", 46.9, 7.4).unwrap();

    // Lock and unlock with the duress PIN again.
    assert_eq!(wb.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);

    assert_eq!(
        names(wb.list_groups().unwrap(), |g| g.name().to_string()),
        ["Work"],
        "a group that vanished after a lock would give duress mode away"
    );
    assert_eq!(
        names(wb.list_tags().unwrap(), |t| t.name.clone()),
        ["books"]
    );
    assert_eq!(
        names(wb.list_places().unwrap(), |p| p.name.clone()),
        ["Bern"]
    );
}

// @scenario: duress_mode :: The duress setup is invisible in duress mode
#[test]
fn the_real_vocabulary_stays_untouched_and_the_duress_one_goes_with_duress() {
    let mut wb = alice_under_duress();
    wb.create_group("Work").unwrap();
    wb.create_tag("books").unwrap();

    assert_eq!(wb.authenticate(APP_PIN).unwrap(), AuthMode::Normal);
    assert!(wb.list_groups().unwrap().is_empty());
    assert!(wb.list_tags().unwrap().is_empty());

    wb.disable_duress().unwrap();
    wb.setup_duress_password(DURESS_PIN).unwrap();
    assert_eq!(wb.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);
    assert!(
        wb.list_groups().unwrap().is_empty(),
        "disabling duress discards what its sessions made"
    );
    assert!(wb.list_tags().unwrap().is_empty());
}
