// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! ContactDetail exchange-place section (ADR-051, Phase 4c slice 1):
//! engine rendering + the AppEngine intercept that names / clears a
//! contact's recorded exchange location through a real `Vauchi`.
//!
//! Location *capture* at exchange time (the `LocationRequest` seam) lands
//! separately; here the location is seeded via `Vauchi::set_exchange_location`
//! (its core entry point).

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, ContactDetailEngine, ContactPlace, Field, Item,
    UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;

fn sample_contact() -> Item {
    Item {
        id: "c1".into(),
        name: "Alice".into(),
        subtitle: None,
        initials: "A".into(),
        status: None,
        actions: vec![],
        a11y: None,
    }
}

fn engine_with_place(place: Option<ContactPlace>) -> ContactDetailEngine {
    ContactDetailEngine::new(sample_contact(), Vec::<Field>::new(), String::new())
        .with_exchange_place(place)
}

fn place_label(screen: &vauchi_app::ui::ScreenModel) -> Option<String> {
    screen.components.iter().find_map(|c| match c {
        Component::Text { id, content, .. } if id == "exchange_place_label" => {
            Some(content.clone())
        }
        _ => None,
    })
}

fn has_component_id(screen: &vauchi_app::ui::ScreenModel, want: &str) -> bool {
    screen.components.iter().any(|c| match c {
        Component::Text { id, .. }
        | Component::TextInput { id, .. }
        | Component::ActionList { id, .. } => id == want,
        _ => false,
    })
}

// ── Engine-level ───────────────────────────────────────────────────────────

// @internal
#[test]
fn named_place_renders_met_at_label_and_clear_action() {
    let screen = engine_with_place(Some(ContactPlace {
        name: Some("Anchor Bar".into()),
    }))
    .current_screen();
    assert_eq!(place_label(&screen).as_deref(), Some("Met at Anchor Bar"));
    assert!(
        has_component_id(&screen, "name_place"),
        "rename input present"
    );
    assert!(
        has_component_id(&screen, "place_actions"),
        "clear action present"
    );
}

// @internal
#[test]
fn unnamed_location_renders_generic_label() {
    let screen = engine_with_place(Some(ContactPlace { name: None })).current_screen();
    assert_eq!(
        place_label(&screen).as_deref(),
        Some("Exchange location recorded")
    );
}

// @internal
#[test]
fn no_location_renders_no_place_components() {
    let screen = engine_with_place(None).current_screen();
    assert!(place_label(&screen).is_none());
    assert!(!has_component_id(&screen, "name_place"));
    assert!(!has_component_id(&screen, "place_actions"));
    assert!(!has_component_id(&screen, "place_suggestions"));
}

// @internal
#[test]
fn place_query_renders_suggestions() {
    let mut engine = engine_with_place(Some(ContactPlace { name: None }));
    engine.set_place_query("anch".into(), vec!["Anchor Bar".into(), "Anchorage".into()]);
    let screen = engine.current_screen();
    let ids: Vec<String> = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ActionList { id, items } if id == "place_suggestions" => Some(items),
            _ => None,
        })
        .map(|items| items.iter().map(|i| i.id.clone()).collect())
        .unwrap_or_default();
    assert_eq!(ids, vec!["name_place:Anchor Bar", "name_place:Anchorage"]);
}

// ── Intercept / integration ────────────────────────────────────────────────

fn engine_with_located_contact() -> (AppEngine, String) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Me").unwrap();
    vauchi
        .import_contacts_from_vcf(b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob\r\nEND:VCARD\r\n")
        .unwrap();
    let cid = vauchi.list_contacts().unwrap()[0].id().to_string();
    // Seed a recorded exchange location (the capture seam lands separately).
    vauchi.set_exchange_location(&cid, 47.37, 8.54).unwrap();
    (AppEngine::new(vauchi), cid)
}

fn screen_of(result: ActionResult) -> vauchi_app::ui::ScreenModel {
    match result {
        ActionResult::UpdateScreen(s) | ActionResult::NavigateTo(s) => s,
        other => panic!("expected a screen-bearing result, got: {other:?}"),
    }
}

// @internal
#[test]
fn detail_shows_recorded_location_then_names_and_clears_it() {
    let (mut engine, cid) = engine_with_located_contact();
    let screen = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: cid.clone(),
    });
    assert_eq!(
        place_label(&screen).as_deref(),
        Some("Exchange location recorded"),
        "seeded location renders as unnamed"
    );

    // Name it (autocomplete-or-create) → "Met at Anchor Bar".
    let named = screen_of(engine.handle_action(UserAction::ActionPressed {
        action_id: "name_place:Anchor Bar".into(),
    }));
    assert_eq!(place_label(&named).as_deref(), Some("Met at Anchor Bar"));

    // Clear it → no place section.
    let cleared = screen_of(engine.handle_action(UserAction::ActionPressed {
        action_id: "clear_exchange_place".into(),
    }));
    assert!(
        place_label(&cleared).is_none(),
        "cleared location removes the place section"
    );
}

// @internal
#[test]
fn typing_a_place_name_suggests_the_existing_vocabulary() {
    let (mut engine, cid) = engine_with_located_contact();
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: cid.clone(),
    });
    // Seed the vocabulary by naming, clearing the query path: name once.
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "name_place:Anchor Bar".into(),
    });

    // Now type a matching prefix → the existing place is suggested.
    let screen = screen_of(engine.handle_action(UserAction::TextChanged {
        component_id: "name_place".into(),
        value: "anch".into(),
    }));
    let ids: Vec<String> = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ActionList { id, items } if id == "place_suggestions" => Some(items),
            _ => None,
        })
        .map(|items| items.iter().map(|i| i.id.clone()).collect())
        .unwrap_or_default();
    assert_eq!(ids, vec!["name_place:Anchor Bar"]);
}
