// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `AppEngine::invalidate_screen` must refresh the LIVE engine when the
//! invalidated screen is the one currently displayed — cache eviction
//! alone leaves the user parked on a stale snapshot until they navigate
//! away and back (the parked-on-Contacts hole of
//! 2026-07-01-android-contacts-list-stale-after-mutation; also afflicts
//! the sync-apply path fixed in 5d13a463).

use vauchi_app::ui::{AppEngine, AppScreen, Component, WorkflowEngine};
use vauchi_core::ImportSource;
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn add_contact_named(engine: &AppEngine, name: &str) {
    let contact = Contact::from_import(ContactCard::new(name), ImportSource::VcardFile, None, 0);
    engine.vauchi().add_contact(contact).unwrap();
}

fn contact_names(engine: &AppEngine) -> Vec<String> {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == "contacts" => Some(items.clone()),
            _ => None,
        })
        .map(|items| items.iter().map(|i| i.name.clone()).collect())
        .unwrap_or_default()
}

/// Invalidating the screen the user is parked on rebuilds the live
/// engine — the very next `current_screen()` reflects the mutation, no
/// navigate-away-and-back round trip required.
// @internal
#[test]
fn invalidate_of_current_screen_rebuilds_live_engine() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Contacts);
    assert_eq!(
        contact_names(&engine),
        Vec::<String>::new(),
        "precondition: contacts list starts empty"
    );

    add_contact_named(&engine, "Bob");
    engine.invalidate_screen(&AppScreen::Contacts);

    assert_eq!(
        contact_names(&engine),
        vec!["Bob".to_string()],
        "invalidating the CURRENT screen must rebuild the live engine, \
         not only evict the cache"
    );
}

/// Invalidating a non-current screen keeps evicting the cache so the
/// next navigation builds fresh (pins the pre-existing behavior).
// @internal
#[test]
fn invalidate_of_cached_screen_evicts_so_next_visit_is_fresh() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Contacts);
    engine.navigate_to(AppScreen::MyInfo);

    add_contact_named(&engine, "Bob");
    engine.invalidate_screen(&AppScreen::Contacts);
    engine.navigate_to(AppScreen::Contacts);

    assert_eq!(
        contact_names(&engine),
        vec!["Bob".to_string()],
        "a cached invalidated screen must rebuild fresh on next visit"
    );
}
