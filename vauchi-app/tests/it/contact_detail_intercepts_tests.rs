// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `AppEngine` tag intercepts on the ContactDetail
//! screen (ADR-051 contact annotations, Phase 4a).
//!
//! Add/remove/suggest persistence lives in the AppEngine intercept (it
//! needs `Vauchi`), not in `ContactDetailEngine`. These tests drive a real
//! in-memory `Vauchi` so they exercise the full round-trip: action →
//! intercept → core API → storage → screen reload.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn vauchi_with_contact() -> (Vauchi, String) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Test User").unwrap();
    let vcf = b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob\r\nEND:VCARD\r\n";
    vauchi.import_contacts_from_vcf(vcf).unwrap();
    let contact_id = vauchi.list_contacts().unwrap()[0].id().to_string();
    (vauchi, contact_id)
}

/// Items of the flat `contact_tags` ActionList on the given screen.
fn tag_rows(screen: &vauchi_app::ui::ScreenModel) -> Vec<(String, String)> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ActionList { id, items } if id == "contact_tags" => Some(items),
            _ => None,
        })
        .map(|items| {
            items
                .iter()
                .map(|i| (i.id.clone(), i.label.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn suggestion_ids(screen: &vauchi_app::ui::ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ActionList { id, items } if id == "tag_suggestions" => Some(items),
            _ => None,
        })
        .map(|items| items.iter().map(|i| i.id.clone()).collect())
        .unwrap_or_default()
}

fn screen_of(result: ActionResult) -> vauchi_app::ui::ScreenModel {
    match result {
        ActionResult::UpdateScreen(s) | ActionResult::NavigateTo(s) => s,
        other => panic!("expected a screen-bearing result, got: {other:?}"),
    }
}

// @internal
#[test]
fn add_tag_action_persists_and_renders_on_contact_detail() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: contact_id.clone(),
    });

    // No tags initially.
    assert!(
        tag_rows(&engine.current_screen()).is_empty(),
        "contact starts with no tags"
    );

    let screen = screen_of(engine.handle_action(UserAction::ActionPressed {
        action_id: "add_tag:climbing".into(),
    }));

    assert_eq!(screen.screen_id, "contact_detail");
    let rows = tag_rows(&screen);
    assert_eq!(rows.len(), 1, "exactly one tag after add, got {rows:?}");
    assert_eq!(rows[0].1, "climbing", "tag label rendered");
    assert!(
        rows[0].0.starts_with("remove_tag:"),
        "tag row carries a remove action id, got {}",
        rows[0].0
    );
}

// @internal
#[test]
fn add_tag_is_autocomplete_or_create_and_dedups_by_name() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: contact_id.clone(),
    });

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "add_tag:climbing".into(),
    });
    // Adding the same name again must not create a duplicate row.
    let screen = screen_of(engine.handle_action(UserAction::ActionPressed {
        action_id: "add_tag:climbing".into(),
    }));

    let rows = tag_rows(&screen);
    assert_eq!(
        rows.len(),
        1,
        "re-adding the same tag name must dedup, got {rows:?}"
    );
}

// @internal
#[test]
fn typing_add_tag_query_shows_matching_suggestions() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: contact_id.clone(),
    });

    // Seed the tag vocabulary.
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "add_tag:climbing".into(),
    });

    let screen = screen_of(engine.handle_action(UserAction::TextChanged {
        component_id: "add_tag".into(),
        value: "cl".into(),
    }));

    assert_eq!(
        suggestion_ids(&screen),
        vec!["add_tag:climbing".to_string()],
        "typing a matching prefix surfaces the existing tag as a suggestion"
    );
}

// @internal
#[test]
fn empty_query_renders_no_suggestions() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: contact_id.clone(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "add_tag:climbing".into(),
    });

    let screen = screen_of(engine.handle_action(UserAction::TextChanged {
        component_id: "add_tag".into(),
        value: String::new(),
    }));

    assert!(
        suggestion_ids(&screen).is_empty(),
        "an empty query must clear the suggestion list"
    );
}

// @internal
#[test]
fn remove_tag_action_persists_and_clears_the_row() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: contact_id.clone(),
    });

    let added = screen_of(engine.handle_action(UserAction::ActionPressed {
        action_id: "add_tag:climbing".into(),
    }));
    let remove_id = tag_rows(&added)[0].0.clone();
    assert!(remove_id.starts_with("remove_tag:"));

    let screen = screen_of(engine.handle_action(UserAction::ActionPressed {
        action_id: remove_id,
    }));

    assert!(
        tag_rows(&screen).is_empty(),
        "removing the only tag clears the list, got {:?}",
        tag_rows(&screen)
    );
}
