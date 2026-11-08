// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::Value;
use vauchi_app::ui::AppEngine;
use vauchi_core::{
    ActionSpec, Command, Event, InputMode, MotionPreference, SurfaceId, SurfaceSpec,
    api::Vauchi,
    contact_card::{ContactField, FieldType},
};

fn card(has_field: bool, width: u32) -> (AppEngine, Vec<Command>) {
    let mut vauchi = Vauchi::in_memory().expect("in-memory Core");
    vauchi.create_identity("First card").expect("identity");
    if has_field {
        let mut card = vauchi.own_card().expect("own card").expect("identity card");
        card.add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "probe@example.invalid",
            0,
        ))
        .expect("first field");
        vauchi.update_own_card(&card).expect("save card");
    }
    let mut app = AppEngine::new(vauchi);
    let mut commands = app.initial_commands().expect("initial presentation");
    commands.extend(
        app.dispatch(Event::PresentationEnvironmentChanged {
            available_width: width,
            available_height: 720,
            input_modes: if width < 600 {
                vec![InputMode::Touch]
            } else {
                vec![InputMode::Pointer, InputMode::Keyboard]
            },
            motion: MotionPreference::Full,
            color_scheme: None,
        })
        .expect("presentation environment"),
    );
    (app, commands)
}

fn card_surface(commands: &[Command]) -> &SurfaceSpec {
    let (id, primary) = commands
        .iter()
        .rev()
        .find_map(|command| match command {
            Command::SetContextBar {
                surface_id, bar, ..
            } => bar.primary.as_ref().map(|action| (surface_id, action)),
            _ => None,
        })
        .expect("primary action");
    assert_eq!(primary.label, "Exchange Now");
    assert!(primary.enabled);
    commands
        .iter()
        .rev()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } if &surface.surface_id == id => Some(surface),
            _ => None,
        })
        .expect("card surface")
}

fn add_entry(value: &Value) -> Option<ActionSpec> {
    match value {
        Value::Object(object) => {
            if object.contains_key("interaction_id")
                && object.get("label") == Some(&Value::from("Add Entry"))
            {
                return Some(serde_json::from_value(value.clone()).expect("prepared action"));
            }
            object.values().find_map(add_entry)
        }
        Value::Array(values) => values.iter().find_map(add_entry),
        _ => None,
    }
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn empty_card_offers_add_entry_in_its_body_without_replacing_exchange() {
    for width in [360, 960] {
        let (mut app, commands) = card(false, width);
        let surface = card_surface(&commands);
        let id: SurfaceId = surface.surface_id.clone();
        let nodes = serde_json::to_value(&surface.nodes).expect("presentation nodes");
        let action = add_entry(&nodes).expect("Add Entry is directly available in the card body");
        assert!(action.enabled);
        app.dispatch(Event::SurfaceActivated {
            surface_id: id.clone(),
        })
        .expect("activate card");
        let next = app
            .dispatch(Event::ActionActivated {
                surface_id: id,
                interaction_id: action.interaction_id,
            })
            .expect("activate Add Entry");
        let titles: Vec<_> = next
            .iter()
            .filter_map(|command| match command {
                Command::ReplaceSurface { surface } => Some(surface.title.as_str()),
                _ => None,
            })
            .collect();
        assert!(titles.contains(&"Add to your card"), "{titles:?}");
    }
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn a_card_with_entries_keeps_the_existing_body_and_exchange_action() {
    let (_, commands) = card(true, 960);
    let surface = card_surface(&commands);
    let nodes = serde_json::to_value(&surface.nodes).expect("presentation nodes");
    assert_eq!(add_entry(&nodes), None);
}
