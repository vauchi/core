// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The navigation overlay does not offer itself as a destination.
//!
//! Every shell opens one flat destination list from the command bar, and
//! Core titles that overlay `nav.more`. Listing the More screen inside it
//! gave a menu called "More" whose last entry was "More" — verified on a
//! Pixel 3a and an iPhone SE, 2026-08-20.

use vauchi_app::ui::AppEngine;
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// The screen ids every shell is offered in the navigation overlay.
fn destination_ids(engine: &AppEngine) -> Vec<String> {
    engine
        .sidebar_items(vauchi_app::i18n::Locale::English)
        .into_iter()
        .map(|tab| tab.id)
        .collect()
}

// @internal
#[test]
fn navigation_destinations_do_not_include_the_overlay_itself() {
    let engine = engine_with_identity();
    let ids = destination_ids(&engine);

    assert!(
        !ids.is_empty(),
        "no navigation destinations, so this test proved nothing"
    );
    assert!(
        !ids.iter().any(|id| id == "more"),
        "the navigation overlay is titled nav.more, so offering `more` inside \
         it is a self-reference; destinations: {ids:?}"
    );
}

/// Screens the More menu is the only in-app route to.
///
/// Deliberately short. Most of what the More menu lists is reachable
/// without it and must not be promoted twice: `archived_contacts` and
/// `contact_duplicates` hang off the Contacts screen's `view_archived`
/// and `find_duplicates`, and `device_replacement` off `setup_new_device`.
/// Only these two have no other route.
const FORMERLY_MORE_ONLY: &[&str] = &["tags", "places"];

// @internal
#[test]
fn destinations_that_only_more_reached_are_offered_directly() {
    let engine = engine_with_identity();
    let ids = destination_ids(&engine);

    for screen in FORMERLY_MORE_ONLY {
        assert!(
            ids.iter().any(|id| id == screen),
            "retiring the More menu leaves {screen:?} with no in-app route; \
             it must be offered in the navigation overlay. destinations: {ids:?}"
        );
    }
}

// @internal
#[test]
fn every_destination_resolves_to_a_screen() {
    let engine = engine_with_identity();
    for id in destination_ids(&engine) {
        assert!(
            vauchi_app::ui::AppScreen::from_screen_id(&id).is_some(),
            "destination {id:?} does not resolve to a screen, so activating \
             it cannot navigate anywhere"
        );
    }
}

// @internal
#[test]
fn navigation_destinations_are_unique() {
    let engine = engine_with_identity();
    let ids = destination_ids(&engine);

    let mut seen = ids.clone();
    seen.sort();
    seen.dedup();
    assert_eq!(
        seen.len(),
        ids.len(),
        "a destination is listed more than once: {ids:?}"
    );
}
