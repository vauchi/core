// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{
    AccessibilitySpec, BindingId, Command, Event, InputValue, PresentationAxis,
    PresentationInputKind, PresentationNode, PresentationTextStyle, PresentationTokens,
    SurfaceLayout, SurfaceSpec,
};

fn binding(id: &str) -> BindingId {
    BindingId::new(id).expect("valid binding identifier")
}

fn surface() -> SurfaceSpec {
    SurfaceSpec {
        surface_id: vauchi_core::SurfaceId::new("profile.edit").unwrap(),
        revision: 7,
        title: "Edit profile".into(),
        subtitle: Some("Changes stay on this device until shared.".into()),
        accessibility_label: "Edit profile screen".into(),
        layout: SurfaceLayout::Scroll,
        tokens: PresentationTokens {
            spacing_small: 8,
            spacing_medium: 16,
            spacing_large: 24,
            corner_radius: 12,
            minimum_target_size: 44,
        },
        nodes: vec![PresentationNode::Group {
            id: Some(binding("profile.fields")),
            label: Some("Profile fields".into()),
            axis: PresentationAxis::Vertical,
            children: vec![
                PresentationNode::Text {
                    id: None,
                    content: "Name".into(),
                    style: PresentationTextStyle::Heading,
                    accessibility: AccessibilitySpec::label("Name"),
                },
                PresentationNode::Input {
                    binding_id: binding("profile.name"),
                    label: "Display name".into(),
                    value: "Ada".into(),
                    placeholder: Some("Name".into()),
                    input_kind: PresentationInputKind::Text,
                    max_length: Some(80),
                    validation_error: None,
                    enabled: true,
                    accessibility: AccessibilitySpec::label("Display name"),
                },
            ],
            accessibility: AccessibilitySpec::label("Profile fields"),
        }],
    }
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn atomic_surface_command_round_trips_with_prepared_content() {
    let command = Command::ReplaceSurface { surface: surface() };

    let encoded = serde_json::to_vec(&command).expect("serialize surface command");
    let decoded: Command = serde_json::from_slice(&encoded).expect("decode surface command");

    assert_eq!(decoded, command);
    assert!(
        encoded
            .windows(b"Edit profile".len())
            .any(|w| w == b"Edit profile")
    );
    assert!(
        encoded
            .windows(b"Display name".len())
            .any(|w| w == b"Display name")
    );
}

// @scenario: generic_presentation_protocol.feature :: User interaction returns as an opaque event
#[test]
fn raw_value_observation_preserves_opaque_binding_and_value() {
    let event = Event::ValueChanged {
        surface_id: vauchi_core::SurfaceId::new("profile.edit").unwrap(),
        binding_id: binding("profile.name"),
        value: InputValue::Text("Ada Lovelace".into()),
    };

    let encoded = serde_json::to_vec(&event).expect("serialize value event");
    let decoded: Event = serde_json::from_slice(&encoded).expect("decode value event");

    assert_eq!(decoded, event);
}

// @internal
#[test]
fn atomic_surface_revision_survives_the_wire_round_trip() {
    let encoded = serde_json::to_vec(&surface()).expect("serialize surface");
    let decoded: SurfaceSpec = serde_json::from_slice(&encoded).expect("decode surface");

    assert_eq!(decoded.revision, 7);
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn presentation_identifiers_reject_control_characters() {
    assert!(BindingId::new("profile\nname").is_err());
}

/// A row may carry an info action beside its controls (vauchi/private#479).
/// A batch from an older Core has no such key and decodes as before; a row
/// without one is sent without the key, so older shells see nothing new.
// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn a_row_decodes_with_and_without_an_info_action() {
    let without = serde_json::json!({
        "title": "Home address", "subtitle": null, "detail": null, "icon_token": null,
        "image_data": null, "fallback_text": null, "selected": false, "enabled": true,
        "activation": null, "secondary_actions": [], "controls": [],
        "accessibility": {"label": "Home address", "description": null}
    });
    let row: vauchi_core::PresentationRow =
        serde_json::from_value(without.clone()).expect("a row without info decodes");
    assert_eq!(row.info, None);
    let encoded = serde_json::to_value(&row).expect("serialize row");
    assert!(
        encoded.get("info").is_none(),
        "an absent info action is left off the wire: {encoded}"
    );

    let mut with = without;
    with["info"] = serde_json::json!({
        "interaction_id": "surface.7.interaction.3", "label": "Info",
        "accessibility_label": "About Home address", "icon_token": null,
        "enabled": true, "shortcut": null
    });
    let row: vauchi_core::PresentationRow =
        serde_json::from_value(with).expect("a row with info decodes");
    let info = row.info.expect("the info action");
    assert_eq!(info.label, "Info");
    assert_eq!(info.accessibility_label, "About Home address");
    assert_eq!(info.interaction_id.as_str(), "surface.7.interaction.3");
}
