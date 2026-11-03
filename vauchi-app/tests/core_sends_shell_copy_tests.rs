// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Core sends the copy a display shell used to look up itself (ADR-066
//! "commands are complete", private#1029 4d.7d, owner 2026-11-03): the
//! window title, a searchable list's placeholder, and a QR capture's
//! placeholder and submit label.

use vauchi_app::ui::{AppEngine, Component, Item, PreparedSurface, ScreenModel};
use vauchi_core::{
    Command, Event, InputMode, MotionPreference, PresentationNode, SurfaceId, api::Vauchi,
};

fn projected(component: Component) -> PresentationNode {
    let screen = ScreenModel::new("probe", "Probe", vec![component], vec![]);
    let prepared = PreparedSurface::from_screen(SurfaceId::new("probe").unwrap(), 1, &screen)
        .expect("supported generic projection");
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("atomic surface command");
    };
    surface.nodes.into_iter().next().expect("one node")
}

fn list(searchable: bool) -> Component {
    Component::List {
        id: "contacts".into(),
        items: vec![Item {
            id: "contact_1".into(),
            name: "Ada".into(),
            subtitle: None,
            initials: "A".into(),
            status: None,
            actions: vec![],
            a11y: None,
        }],
        searchable,
        total_count: 0,
        offset: 0,
        window: 0,
    }
}

fn qr(mode: &str) -> Component {
    serde_json::from_value(serde_json::json!({
        "QrCode": { "id": "code", "data": "payload", "mode": mode, "label": null }
    }))
    .expect("a QR component")
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn the_presentation_profile_carries_the_window_title() {
    let mut engine = AppEngine::new(Vauchi::in_memory().expect("in-memory Core"));
    engine.initial_commands().expect("initial commands");
    let commands = engine
        .dispatch(Event::PresentationEnvironmentChanged {
            available_width: 960,
            available_height: 720,
            input_modes: vec![InputMode::Pointer, InputMode::Keyboard],
            motion: MotionPreference::Full,
            color_scheme: None,
        })
        .expect("environment event");

    let titles: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::SetPresentationProfile { profile } => Some(profile.window_title.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(titles, [Some("Vauchi".to_owned())]);
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn a_searchable_list_carries_its_placeholder_and_a_plain_one_none() {
    let PresentationNode::List {
        search_placeholder, ..
    } = projected(list(true))
    else {
        panic!("a list");
    };
    assert_eq!(search_placeholder.as_deref(), Some("Search"));

    let PresentationNode::List {
        search_placeholder, ..
    } = projected(list(false))
    else {
        panic!("a list");
    };
    assert_eq!(search_placeholder, None);
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn a_qr_capture_carries_its_field_copy_and_a_display_code_none() {
    let PresentationNode::Qr {
        capture_placeholder,
        capture_submit_label,
        ..
    } = projected(qr("Scan"))
    else {
        panic!("a QR node");
    };
    assert_eq!(capture_placeholder.as_deref(), Some("Paste QR data here"));
    assert_eq!(capture_submit_label.as_deref(), Some("Submit"));

    let PresentationNode::Qr {
        capture_placeholder,
        capture_submit_label,
        ..
    } = projected(qr("Display"))
    else {
        panic!("a QR node");
    };
    assert_eq!((capture_placeholder, capture_submit_label), (None, None));
}
