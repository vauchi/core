// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Duress mode shows no real groups, tags or places (#468, ADR-032; owner
//! decision 2026-10-01): they read as none, anything made there looks like
//! it worked for the session and never reaches the real ones, and what real
//! contacts are sent is still decided by the real groups.

use super::common::helpers::create_vauchi_with_identity;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::{AuthMode, Contact, SymmetricKey, Vauchi};

const APP_PIN: &str = "app-pin-1234";
const DURESS_PIN: &str = "135790";

struct World {
    wb: Vauchi,
    rita: String,
    field: String,
}

/// A real contact in a real group that grants a real entry, with a tag and
/// a named place; then unlocked with the duress PIN.
fn in_duress_mode() -> World {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password(APP_PIN).unwrap();
    let contact = Contact::from_exchange(
        [7u8; 32],
        ContactCard::new("Rita"),
        SymmetricKey::generate(),
        0,
    );
    let rita = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    let field = ContactField::new(FieldType::Email, "work", "rita-sees@example.org", 0);
    let field_id = field.id().to_string();
    wb.add_own_field(field).unwrap();
    let group = wb.create_group("Family").unwrap();
    wb.add_contact_to_group(group.id(), &rita).unwrap();
    wb.set_group_field_visibility(group.id(), &field_id, true)
        .unwrap();
    wb.add_tag_to_contact(&rita, "climbing").unwrap();
    wb.set_exchange_location(&rita, 47.0, 8.0).unwrap();
    wb.name_exchange_place(&rita, "Zürich").unwrap();
    let met_elsewhere = Contact::from_exchange(
        [8u8; 32],
        ContactCard::new("Ugo"),
        SymmetricKey::generate(),
        0,
    );
    let ugo = met_elsewhere.id().to_string();
    wb.add_contact(met_elsewhere).unwrap();
    wb.set_exchange_location(&ugo, 46.0, 9.0).unwrap();
    wb.setup_duress_password(DURESS_PIN).unwrap();
    assert_eq!(wb.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);
    World {
        wb,
        rita,
        field: field_id,
    }
}

fn names<T>(items: Vec<T>, name: impl Fn(&T) -> String) -> Vec<String> {
    items.iter().map(name).collect()
}

// @scenario: duress_mode :: The duress setup is invisible in duress mode
// @internal
#[test]
fn duress_mode_shows_no_real_groups_tags_or_places() {
    let w = in_duress_mode();
    assert!(w.wb.list_groups().unwrap().is_empty());
    assert!(w.wb.list_tags().unwrap().is_empty());
    assert!(w.wb.list_places().unwrap().is_empty());
    assert!(w.wb.place_contact_counts().unwrap().is_empty());
    assert!(w.wb.unnamed_exchange_spots().unwrap().is_empty());
}

// @scenario: duress_mode :: Changes made in duress mode look like they worked
// @internal
#[test]
fn what_is_made_in_duress_mode_stays_in_the_session() {
    let mut w = in_duress_mode();
    let group = w.wb.create_group("Work").unwrap();
    w.wb.create_tag("books").unwrap();
    w.wb.create_named_place("Bern", 46.9, 7.4).unwrap();
    assert_eq!(
        names(w.wb.list_groups().unwrap(), |g| g.name().to_string()),
        ["Work"]
    );
    assert_eq!(
        names(w.wb.list_tags().unwrap(), |t| t.name.clone()),
        ["books"]
    );
    assert_eq!(
        names(w.wb.list_places().unwrap(), |p| p.name.clone()),
        ["Bern"]
    );
    w.wb.delete_group(group.id()).unwrap();

    assert_eq!(w.wb.authenticate(APP_PIN).unwrap(), AuthMode::Normal);
    assert_eq!(
        names(w.wb.list_groups().unwrap(), |g| g.name().to_string()),
        ["Family"]
    );
    assert_eq!(
        names(w.wb.list_tags().unwrap(), |t| t.name.clone()),
        ["climbing"]
    );
    assert_eq!(
        names(w.wb.list_places().unwrap(), |p| p.name.clone()),
        ["Zürich"]
    );
    assert_eq!(
        w.wb.place_contact_counts()
            .unwrap()
            .into_values()
            .collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(w.wb.unnamed_exchange_spots().unwrap().len(), 1);
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
// Duress sync keeps sending realistic traffic to real contacts; what they
// receive must still follow the real groups, or real contacts would see
// entries disappear while the owner is coerced.
// @internal
#[test]
fn real_contacts_still_see_what_the_real_groups_grant() {
    let w = in_duress_mode();
    assert!(
        w.wb.get_effective_field_visibility(&w.rita, &w.field)
            .unwrap(),
        "Family still grants Rita the entry during a duress session"
    );
}
