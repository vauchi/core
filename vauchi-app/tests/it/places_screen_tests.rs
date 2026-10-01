// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Places screen (#467, design pass #419 item 9; owner decisions
//! 2026-10-01): each named place says how many contacts you met there, and
//! the spots nobody has named yet are offered to name, described by who you
//! met there so each row is distinct, also to a screen reader.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, PlaceSummary, PlacesEngine, ScreenModel,
    UnnamedPlace, UserAction, WorkflowEngine,
};
use vauchi_core::{Contact, ContactCard, SymmetricKey, Vauchi};

const INTRO: &str = "Where you exchanged cards, named by you. Places stay on your devices.";
const EMPTY: &str = "No places yet. When an exchange records where it happened, the place \
appears here for you to name.";

fn texts(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::Text { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

fn rows(screen: &ScreenModel, list: &str) -> Vec<(String, Option<String>)> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == list => Some(
                items
                    .iter()
                    .map(|i| (i.name.clone(), i.subtitle.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn zurich(met: usize) -> PlaceSummary {
    PlaceSummary {
        id: "p1".into(),
        name: "Zürich".into(),
        met_count: met,
    }
}

fn unnamed(anchor: &str, met: &[&str]) -> UnnamedPlace {
    UnnamedPlace {
        anchor_contact_id: anchor.into(),
        met: met.iter().map(|n| (*n).to_string()).collect(),
    }
}

// @internal
#[test]
fn a_named_place_says_how_many_you_met_there() {
    let screen = PlacesEngine::new(vec![zurich(4)]).current_screen();
    assert_eq!(texts(&screen), [INTRO]);
    assert_eq!(
        rows(&screen, "places"),
        [("Zürich".into(), Some("4 contacts met here".into()))]
    );
    let one = PlacesEngine::new(vec![zurich(1)]).current_screen();
    assert_eq!(
        rows(&one, "places")[0].1.as_deref(),
        Some("1 contact met here")
    );
    let none = PlacesEngine::new(vec![zurich(0)]).current_screen();
    assert_eq!(
        rows(&none, "places")[0].1.as_deref(),
        Some("No contacts met here yet")
    );
}

// @internal
#[test]
fn unnamed_places_are_described_by_who_you_met_there() {
    let screen = PlacesEngine::new(vec![])
        .with_unnamed(vec![
            unnamed("c-dan", &["Dan", "Eve"]),
            unnamed("c-fay", &["Fay"]),
            unnamed("c-gus", &["Gus", "Hal", "Ida"]),
        ])
        .current_screen();
    assert_eq!(
        rows(&screen, "unnamed_places"),
        [
            (
                "Unnamed place".into(),
                Some("Where you met Dan and Eve · tap to name".into())
            ),
            (
                "Unnamed place".into(),
                Some("Where you met Fay · tap to name".into())
            ),
            (
                "Unnamed place".into(),
                Some("Where you met Gus and 2 others · tap to name".into())
            ),
        ]
    );
}

// @internal
#[test]
fn an_unnamed_place_opens_the_naming_form() {
    let mut engine = PlacesEngine::new(vec![]).with_unnamed(vec![unnamed("c-dan", &["Dan"])]);
    assert_eq!(
        engine.handle_action(UserAction::ListItemSelected {
            component_id: "unnamed_places".into(),
            item_id: "c-dan".into(),
        }),
        ActionResult::ShowFormDialog {
            dialog_type: "name_place".into(),
            context_id: Some("c-dan".into()),
        }
    );
}

// @internal
#[test]
fn no_places_says_how_they_appear() {
    let screen = PlacesEngine::new(vec![]).current_screen();
    assert_eq!(texts(&screen), [INTRO, EMPTY]);
    assert!(rows(&screen, "places").is_empty());
    assert!(rows(&screen, "unnamed_places").is_empty());
}

// @internal
#[test]
fn a_place_delete_reads_as_destructive() {
    let screen = PlacesEngine::new(vec![zurich(4)]).current_screen();
    let destructive = screen.components.iter().find_map(|c| match c {
        Component::List { id, items, .. } if id == "places" => items[0]
            .actions
            .iter()
            .find(|a| a.id == "request_delete")
            .map(|a| a.destructive),
        _ => None,
    });
    assert_eq!(destructive, Some(true));
}

const LAT: f64 = 47.3769;
const LON: f64 = 8.5417;

fn met_at(vauchi: &Vauchi, name: &str, lat: f64) -> String {
    let mut pk = [0u8; 32];
    for (i, b) in name.bytes().enumerate().take(32) {
        pk[i] = b;
    }
    let contact = Contact::from_exchange(pk, ContactCard::new(name), SymmetricKey::generate(), 0);
    let id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    vauchi.set_exchange_location(&id, lat, LON).unwrap();
    id
}

// @internal
#[test]
fn naming_an_unnamed_place_names_everyone_met_there() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    let bob = met_at(&vauchi, "Bob", LAT);
    vauchi.name_exchange_place(&bob, "Zürich").unwrap();
    let dan = met_at(&vauchi, "Dan", LAT + 0.01);
    let _eve = met_at(&vauchi, "Eve", LAT + 0.0101);

    let mut engine = AppEngine::new(vauchi);
    let places = engine.navigate_to(AppScreen::Places);
    assert_eq!(
        rows(&places, "places"),
        [("Zürich".into(), Some("1 contact met here".into()))]
    );
    assert_eq!(
        rows(&places, "unnamed_places"),
        [(
            "Unnamed place".into(),
            Some("Where you met Dan and Eve · tap to name".into())
        )]
    );

    let opened = engine.handle_action(UserAction::ListItemSelected {
        component_id: "unnamed_places".into(),
        item_id: dan,
    });
    let ActionResult::NavigateTo(form) = opened else {
        panic!("the naming form opens, got {opened:?}");
    };
    assert_eq!(form.title, "Name This Place");
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "place_name".into(),
        value: "Bundesplatz".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    let places = engine.navigate_to(AppScreen::Places);
    let mut named = rows(&places, "places");
    named.sort();
    assert_eq!(
        named,
        [
            ("Bundesplatz".into(), Some("2 contacts met here".into())),
            ("Zürich".into(), Some("1 contact met here".into())),
        ]
    );
    assert!(rows(&places, "unnamed_places").is_empty());
}
