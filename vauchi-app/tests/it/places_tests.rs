// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Places management screen (ADR-051, Phase 4c): engine render + the
//! delete-confirm state machine, plus More→Places navigation and the
//! delete intercept driven through a real `Vauchi`.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, PlaceSummary, PlacesEngine, UserAction,
    WorkflowEngine,
};
use vauchi_core::api::Vauchi;

fn sample() -> Vec<PlaceSummary> {
    vec![
        PlaceSummary {
            id: "p1".into(),
            name: "Anchor Bar".into(),
        },
        PlaceSummary {
            id: "p2".into(),
            name: "Zurich HB".into(),
        },
    ]
}

fn place_rows(screen: &vauchi_app::ui::ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == "places" => Some(items),
            _ => None,
        })
        .map(|items| items.iter().map(|i| i.name.clone()).collect())
        .unwrap_or_default()
}

fn has_delete_confirm(screen: &vauchi_app::ui::ScreenModel) -> bool {
    screen.components.iter().any(|c| {
        matches!(c, Component::InlineConfirm { id, destructive, .. }
            if id == "delete_place" && *destructive)
    })
}

fn screen_of(result: ActionResult) -> vauchi_app::ui::ScreenModel {
    match result {
        ActionResult::UpdateScreen(s) | ActionResult::NavigateTo(s) => s,
        other => panic!("expected a screen-bearing result, got: {other:?}"),
    }
}

// ── Engine-level ───────────────────────────────────────────────────────────

// @internal
#[test]
fn renders_places_with_delete_action() {
    let screen = PlacesEngine::new(sample()).current_screen();
    assert_eq!(screen.screen_id, "places");
    assert_eq!(place_rows(&screen), vec!["Anchor Bar", "Zurich HB"]);
}

// @internal
#[test]
fn request_delete_arms_confirm_naming_the_place() {
    let mut engine = PlacesEngine::new(sample());
    let _ = engine.handle_action(UserAction::ListItemAction {
        component_id: "places".into(),
        item_id: "p1".into(),
        action_id: "request_delete".into(),
    });
    assert_eq!(engine.pending_delete_id(), Some("p1"));
    let screen = engine.current_screen();
    assert!(has_delete_confirm(&screen));
    let warning = screen.components.iter().find_map(|c| match c {
        Component::InlineConfirm { id, warning, .. } if id == "delete_place" => {
            Some(warning.clone())
        }
        _ => None,
    });
    assert!(warning.unwrap().contains("Anchor Bar"));
}

// @internal
#[test]
fn confirm_delete_drops_the_row() {
    let mut engine = PlacesEngine::new(sample());
    let _ = engine.handle_action(UserAction::ListItemAction {
        component_id: "places".into(),
        item_id: "p1".into(),
        action_id: "request_delete".into(),
    });
    engine.confirm_delete();
    assert_eq!(engine.pending_delete_id(), None);
    assert_eq!(place_rows(&engine.current_screen()), vec!["Zurich HB"]);
}

// ── Intercept / integration ────────────────────────────────────────────────

/// AppEngine with one named place (seeded via the core API).
fn engine_with_a_place() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Me").unwrap();
    vauchi
        .create_named_place("Anchor Bar", 47.37, 8.54)
        .unwrap();
    AppEngine::new(vauchi)
}

// @internal
#[test]
fn confirm_delete_place_removes_it_via_intercept() {
    let mut engine = engine_with_a_place();
    let screen = engine.navigate_to(AppScreen::Places);
    assert_eq!(place_rows(&screen), vec!["Anchor Bar"]);
    let place_id = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == "places" => Some(items[0].id.clone()),
            _ => None,
        })
        .unwrap();

    let _ = engine.handle_action(UserAction::ListItemAction {
        component_id: "places".into(),
        item_id: place_id,
        action_id: "request_delete".into(),
    });
    let after = screen_of(engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_delete_place".into(),
    }));
    assert!(place_rows(&after).is_empty(), "place removed after confirm");

    let reloaded = engine.navigate_to(AppScreen::Places);
    assert!(
        place_rows(&reloaded).is_empty(),
        "delete persisted across re-navigation"
    );
}
