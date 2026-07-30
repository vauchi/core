// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::{
    ActionListItem, Component, InputType, PreparedSurface, ScreenModel, TextStyle, UserAction,
};
use vauchi_core::{
    Command, Event, InputValue, PresentationInputKind, PresentationNode, PresentationTextStyle,
    SurfaceId,
};

fn screen() -> ScreenModel {
    ScreenModel::new(
        "profile.edit",
        "Edit profile",
        vec![
            Component::Text {
                id: "intro".into(),
                content: "Choose how your name appears.".into(),
                style: TextStyle::Body,
            },
            Component::TextInput {
                id: "display_name".into(),
                label: "Display name".into(),
                value: "Ada".into(),
                placeholder: Some("Name".into()),
                max_length: Some(80),
                validation_error: None,
                input_type: InputType::Text,
                a11y: None,
                info_key: None,
            },
        ],
        vec![],
    )
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn screen_state_projects_to_an_atomic_generic_surface_command() {
    let prepared =
        PreparedSurface::from_screen(SurfaceId::new("profile.edit").unwrap(), 7, &screen())
            .expect("supported generic projection");
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("surface projection must be atomic");
    };

    assert_eq!(surface.title, "Edit profile");
    assert!(matches!(
        &surface.nodes[0],
        PresentationNode::Text {
            content,
            style: PresentationTextStyle::Body,
            ..
        } if content == "Choose how your name appears."
    ));
    assert!(matches!(
        &surface.nodes[1],
        PresentationNode::Input {
            label,
            input_kind: PresentationInputKind::Text,
            ..
        } if label == "Display name"
    ));
}

// @scenario: generic_presentation_protocol.feature :: User interaction returns as an opaque event
#[test]
fn raw_value_event_reduces_to_the_internal_workflow_action() {
    let prepared =
        PreparedSurface::from_screen(SurfaceId::new("profile.edit").unwrap(), 7, &screen())
            .expect("supported generic projection");

    assert_eq!(
        prepared
            .reduce(Event::ValueChanged {
                surface_id: SurfaceId::new("profile.edit").unwrap(),
                binding_id: vauchi_core::BindingId::new("surface.7.display_name").unwrap(),
                value: InputValue::Text("Ada Lovelace".into()),
            })
            .expect("current binding and value type"),
        UserAction::TextChanged {
            component_id: "display_name".into(),
            value: "Ada Lovelace".into(),
        }
    );
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn value_event_for_another_surface_fails_closed() {
    let prepared =
        PreparedSurface::from_screen(SurfaceId::new("profile.edit").unwrap(), 7, &screen())
            .expect("supported generic projection");

    assert!(
        prepared
            .reduce(Event::ValueChanged {
                surface_id: SurfaceId::new("profile.other").unwrap(),
                binding_id: vauchi_core::BindingId::new("surface.7.display_name").unwrap(),
                value: InputValue::Text("Mallory".into()),
            })
            .is_err()
    );
}

// @scenario: generic_presentation_protocol.feature :: Release contains only the generic action system
#[test]
fn every_golden_screen_projects_without_a_legacy_component_escape_hatch() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../vauchi-core/tests/fixtures/golden");
    for entry in std::fs::read_dir(fixtures).expect("golden fixture directory") {
        let path = entry.expect("fixture entry").path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path).expect("read fixture");
        let screen: ScreenModel = serde_json::from_str(&raw)
            .unwrap_or_else(|error| panic!("{} must decode: {error}", path.display()));
        PreparedSurface::from_screen(
            SurfaceId::new(screen.screen_id.clone()).expect("valid fixture surface"),
            1,
            &screen,
        )
        .unwrap_or_else(|error| panic!("{} must project: {error}", path.display()));
    }
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn value_event_from_an_old_surface_revision_fails_closed() {
    let prepared =
        PreparedSurface::from_screen(SurfaceId::new("profile.edit").unwrap(), 7, &screen())
            .expect("supported generic projection");

    assert!(
        prepared
            .reduce(Event::ValueChanged {
                surface_id: SurfaceId::new("profile.edit").unwrap(),
                binding_id: vauchi_core::BindingId::new("surface.6.display_name").unwrap(),
                value: InputValue::Text("Stale".into()),
            })
            .is_err()
    );
}

// @scenario: generic_presentation_protocol.feature :: User interaction returns as an opaque event
#[test]
fn action_list_activation_preserves_the_list_selection_route() {
    let action_screen = ScreenModel::new(
        "settings",
        "Settings",
        vec![Component::ActionList {
            id: "settings.actions".into(),
            items: vec![ActionListItem {
                id: "delete_local_data".into(),
                label: "Delete local data".into(),
                icon: None,
                detail: None,
                a11y: None,
                info_key: None,
            }],
        }],
        vec![],
    );
    let prepared =
        PreparedSurface::from_screen(SurfaceId::new("settings").unwrap(), 9, &action_screen)
            .expect("supported generic projection");
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("atomic surface command");
    };
    let PresentationNode::List { rows, .. } = &surface.nodes[0] else {
        panic!("action list projects as a list");
    };
    let interaction_id = rows[0]
        .activation
        .as_ref()
        .expect("row activation")
        .interaction_id
        .clone();

    assert_eq!(
        prepared
            .reduce(Event::ActionActivated {
                surface_id: SurfaceId::new("settings").unwrap(),
                interaction_id,
            })
            .expect("registered activation"),
        UserAction::ListItemSelected {
            component_id: "settings.actions".into(),
            item_id: "delete_local_data".into(),
        }
    );
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn binding_rejects_a_value_of_the_wrong_generic_type() {
    let prepared =
        PreparedSurface::from_screen(SurfaceId::new("profile.edit").unwrap(), 7, &screen())
            .expect("supported generic projection");

    assert!(
        prepared
            .reduce(Event::ValueChanged {
                surface_id: SurfaceId::new("profile.edit").unwrap(),
                binding_id: vauchi_core::BindingId::new("surface.7.display_name").unwrap(),
                value: InputValue::Boolean(true),
            })
            .is_err()
    );
}
