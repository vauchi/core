// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The navigation overlay does not offer itself as a destination.
//!
//! Every shell opens one flat destination list from the command bar, and
//! Core titles that overlay `nav.more`. Listing the More screen inside it
//! gave a menu called "More" whose last entry was "More" — verified on a
//! Pixel 3a and an iPhone SE, 2026-08-20.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
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

/// Screens the More menu was the only in-app route to, each paired with
/// the route that replaced it.
///
/// Deliberately short. Most of what the More menu listed is reachable
/// without it and must not be promoted twice: `archived_contacts` and
/// `contact_duplicates` hang off the Contacts screen's `view_archived`
/// and `find_duplicates`, and `device_replacement` off `setup_new_device`.
/// Only these two had no other route.
///
/// The overlay *was* that route until the nav was cut to the five primary
/// destinations; a Contacts-screen action is that route now — both are
/// vocabularies the contact book is organized by, so that is where they
/// belong. What this guard has always protected is that these two are
/// reachable **at all** — a menu change orphaned them once — so it names
/// the route it checks rather than assuming the overlay is the only route
/// that can exist. `nav_primary_destinations_tests` runs the same check
/// across every demoted screen, deriving the set from the code; this pair
/// keeps its own guard because it is the pair with the incident behind it.
const FORMERLY_MORE_ONLY: &[(&str, &str)] = &[("tags", "tags"), ("places", "places")];

/// Where the app lands after the user activates the Contacts-screen
/// action `action_id`.
#[track_caller]
fn contacts_action_lands_on(action_id: &str) -> AppScreen {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Contacts);
    let offered = engine
        .current_screen()
        .contextual_actions
        .iter()
        .any(|action| action.id == action_id && action.enabled);
    assert!(
        offered,
        "the contacts screen offers no enabled `{action_id}` action"
    );
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.to_string(),
    });
    engine.current_app_screen().clone()
}

// @internal
#[test]
fn destinations_that_only_more_reached_stay_reachable() {
    for (screen_id, action_id) in FORMERLY_MORE_ONLY {
        let target = AppScreen::from_screen_id(screen_id)
            .unwrap_or_else(|| panic!("`{screen_id}` does not name a screen"));
        assert_eq!(
            contacts_action_lands_on(action_id),
            target,
            "retiring the More menu left {screen_id:?} with no in-app route; \
             the `{action_id}` action on Contacts must reach it"
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
