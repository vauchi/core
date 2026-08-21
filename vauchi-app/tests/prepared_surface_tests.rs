// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::{
    A11y, ActionListItem, Component, DropdownOption, FormDialogEngine, FormDialogType, InputType,
    Item, PreparedSurface, ScreenModel, Section, TextStyle, UserAction, WorkflowEngine,
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
                a11y: None,
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

/// A screen reader speaks `PresentationNode::Text.accessibility.label`
/// (`PresentationNodeRenderer.kt` sets `contentDescription` from it). The
/// projection derives that from `content`, which is right for prose and
/// wrong for a payload: the exchange link carries a 43-character public
/// key, and TalkBack recited it aloud, character group by character
/// group, to everyone in earshot.
// @scenario: generic_presentation_protocol.feature :: A screen can name what a payload is instead of reciting it
#[test]
fn a_supplied_text_label_is_spoken_instead_of_the_payload() {
    const URL: &str = "vauchi://exchange?pk=eYBapim5PowxWTe64UQiPYZX23Zl5jazykMxsI5pikg&n=Plsk";
    let screen = ScreenModel::new(
        "exchange.share",
        "Share Link",
        vec![Component::Text {
            id: "link_url".into(),
            content: URL.into(),
            style: TextStyle::Body,
            a11y: Some(A11y {
                label: Some("Exchange link".into()),
                hint: Some("Share it with the person you are meeting".into()),
                role: None,
            }),
        }],
        vec![],
    );

    let prepared =
        PreparedSurface::from_screen(SurfaceId::new("exchange.share").unwrap(), 3, &screen)
            .expect("supported generic projection");
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("surface projection must be atomic");
    };
    let PresentationNode::Text {
        content,
        accessibility,
        ..
    } = &surface.nodes[0]
    else {
        panic!("expected a Text node, got {:?}", surface.nodes[0]);
    };

    assert_eq!(
        content, URL,
        "the URL must still be rendered — the user has to be able to share it"
    );
    assert_eq!(
        accessibility.label, "Exchange link",
        "a supplied label must reach the shell, which speaks exactly this"
    );
    assert!(
        !accessibility.label.contains("pk="),
        "the spoken label must not recite key material: {}",
        accessibility.label
    );
}

/// The default is unchanged for ordinary prose, which must stay readable.
// @internal
#[test]
fn text_without_a_supplied_label_is_still_spoken_as_written() {
    let prepared =
        PreparedSurface::from_screen(SurfaceId::new("profile.edit").unwrap(), 7, &screen())
            .expect("supported generic projection");
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("surface projection must be atomic");
    };
    let PresentationNode::Text { accessibility, .. } = &surface.nodes[0] else {
        panic!("expected a Text node");
    };
    assert_eq!(accessibility.label, "Choose how your name appears.");
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

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn prepared_choice_carries_every_frontend_decision_in_one_command() {
    let choice_screen = ScreenModel::new(
        "appearance",
        "Choose appearance",
        vec![Component::Dropdown {
            id: "theme".to_owned(),
            label: "Theme".to_owned(),
            selected: Some("dark".to_owned()),
            options: vec![
                DropdownOption {
                    id: "light".to_owned(),
                    label: "Light".to_owned(),
                },
                DropdownOption {
                    id: "dark".to_owned(),
                    label: "Dark".to_owned(),
                },
            ],
            a11y: Some(A11y {
                label: Some("Theme selection".to_owned()),
                hint: Some("Choose the app appearance".to_owned()),
                role: None,
            }),
        }],
        vec![],
    );
    let prepared = PreparedSurface::from_screen(
        SurfaceId::new("appearance").expect("surface id"),
        12,
        &choice_screen,
    )
    .expect("choice projects to a generic surface");
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("choice projection must be atomic");
    };

    assert_eq!(surface.title, "Choose appearance");
    assert_eq!(surface.accessibility_label, "Choose appearance");
    assert_eq!(surface.tokens.spacing_small, 8);
    assert_eq!(surface.tokens.spacing_medium, 16);
    assert_eq!(surface.tokens.spacing_large, 24);
    assert_eq!(surface.tokens.corner_radius, 12);
    assert_eq!(surface.tokens.minimum_target_size, 44);

    let PresentationNode::Choice {
        binding_id,
        label,
        selected,
        options,
        enabled,
        accessibility,
    } = &surface.nodes[0]
    else {
        panic!("dropdown must project as a generic choice");
    };
    assert_eq!(binding_id.as_str(), "surface.12.binding.0");
    assert_eq!(label, "Theme");
    assert_eq!(selected.as_deref(), Some("dark"));
    assert_eq!(
        options
            .iter()
            .map(|option| (option.id.as_str(), option.label.as_str()))
            .collect::<Vec<_>>(),
        vec![("light", "Light"), ("dark", "Dark")]
    );
    assert!(*enabled);
    assert_eq!(accessibility.label, "Theme selection");
    assert_eq!(
        accessibility.description.as_deref(),
        Some("Choose the app appearance")
    );
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

// @scenario: generic_presentation_protocol.feature :: User interaction returns as an opaque event
#[test]
fn prepared_email_activation_advances_the_form_dialog() {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    });
    let prepared = PreparedSurface::from_screen(
        SurfaceId::new("form_add_field").unwrap(),
        3,
        &engine.current_screen(),
    )
    .expect("form picker projects to a generic surface");
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("form picker projects atomically");
    };
    let email_interaction = surface
        .nodes
        .iter()
        .find_map(|node| {
            let PresentationNode::List { rows, .. } = node else {
                return None;
            };
            rows.iter()
                .find(|row| row.title == "Email")
                .and_then(|row| row.activation.as_ref())
                .map(|action| action.interaction_id.clone())
        })
        .expect("Email row activation");

    let action = prepared
        .reduce(Event::ActionActivated {
            surface_id: SurfaceId::new("form_add_field").unwrap(),
            interaction_id: email_interaction,
        })
        .expect("prepared Email activation");
    let _ = engine.handle_action(action);

    assert!(engine.current_screen().components.iter().any(|component| {
        matches!(
            component,
            Component::TextInput { id, .. } if id == "field_value"
        )
    }));
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn a_collection_container_never_announces_its_opaque_component_id() {
    let action_screen = ScreenModel::new(
        "settings",
        "Settings",
        vec![
            Component::ActionList {
                id: "own_entries".into(),
                items: vec![ActionListItem {
                    id: "entry_1".into(),
                    label: "ada@example.com (Email)".into(),
                    icon: None,
                    detail: None,
                    a11y: None,
                    info_key: None,
                }],
            },
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
                searchable: false,
                total_count: 0,
                offset: 0,
                window: 0,
            },
            Component::SectionedActionList {
                id: "more".into(),
                sections: vec![Section {
                    id: "primary".into(),
                    label: "Primary".into(),
                    items: vec![ActionListItem {
                        id: "my_card".into(),
                        label: "My Card".into(),
                        icon: None,
                        detail: None,
                        a11y: None,
                        info_key: None,
                    }],
                }],
            },
        ],
        vec![],
    );
    let prepared =
        PreparedSurface::from_screen(SurfaceId::new("settings").unwrap(), 11, &action_screen)
            .expect("supported generic projection");
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("atomic surface command");
    };

    let PresentationNode::List { accessibility, .. } = &surface.nodes[0] else {
        panic!("action list projects as a list");
    };
    assert_eq!(accessibility.label, "");

    let PresentationNode::List { accessibility, .. } = &surface.nodes[1] else {
        panic!("list projects as a list");
    };
    assert_eq!(accessibility.label, "");

    let PresentationNode::Group {
        accessibility,
        children,
        ..
    } = &surface.nodes[2]
    else {
        panic!("sectioned action list projects as a group");
    };
    assert_eq!(accessibility.label, "");

    let PresentationNode::List { accessibility, .. } = &children[0] else {
        panic!("each section projects as a list");
    };
    assert_eq!(accessibility.label, "Primary");
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

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn a_node_that_is_both_content_and_affordance_carries_one_prepared_name() {
    // Neither AT-SPI toolkit lets an application set an action description,
    // so a shell can surface exactly one name for a widget that is both the
    // content and the way to act on it. Core therefore prepares one, rather
    // than leaving each shell to choose between two
    // (problems/2026-08-21-linux-shells-drop-core-a11y).
    let screen = ScreenModel::new(
        "sync",
        "Sync",
        vec![
            Component::Banner {
                text: "Sync failed".into(),
                action_label: "Retry".into(),
                action_id: "retry_sync".into(),
                a11y: None,
            },
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
                searchable: false,
                total_count: 1,
                offset: 0,
                window: 1,
            },
        ],
        vec![],
    );
    let prepared = PreparedSurface::from_screen(SurfaceId::new("sync").expect("id"), 1, &screen)
        .expect("prepared surface");
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("expected a surface replacement");
    };

    let mut checked = 0;
    for node in &surface.nodes {
        match node {
            PresentationNode::Status {
                activation: Some(action),
                accessibility,
                ..
            } => {
                assert_eq!(
                    action.accessibility_label, accessibility.label,
                    "an activatable banner must announce one name, not one per slot"
                );
                assert_eq!(accessibility.label, "Sync failed");
                checked += 1;
            }
            PresentationNode::List { rows, .. } => {
                for row in rows {
                    let action = row.activation.as_ref().expect("row activation");
                    assert_eq!(
                        action.accessibility_label, row.accessibility.label,
                        "an activatable row must announce one name, not one per slot"
                    );
                    assert_eq!(row.accessibility.label, "Ada");
                    checked += 1;
                }
            }
            _ => {}
        }
    }
    assert_eq!(checked, 2, "expected the banner and the row to be checked");
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn a_rejected_input_announces_the_reason_in_its_accessibility_copy() {
    // Only GTK4 has an error-message relation; Qt has no public equivalent,
    // so the reason has to reach the description every shell already maps
    // (problems/2026-08-21-linux-shells-drop-core-a11y).
    let screen = ScreenModel::new(
        "profile.edit",
        "Edit profile",
        vec![Component::TextInput {
            id: "display_name".into(),
            label: "Display name".into(),
            value: String::new(),
            placeholder: None,
            max_length: None,
            validation_error: Some("Name cannot be empty".into()),
            input_type: InputType::Text,
            a11y: Some(A11y {
                label: Some("Display name".into()),
                hint: Some("Shown to your contacts".into()),
                role: None,
            }),
            info_key: None,
        }],
        vec![],
    );
    let prepared =
        PreparedSurface::from_screen(SurfaceId::new("profile.edit").expect("id"), 1, &screen)
            .expect("prepared surface");
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("expected a surface replacement");
    };

    let Some(PresentationNode::Input {
        accessibility,
        validation_error,
        ..
    }) = surface.nodes.first()
    else {
        panic!("expected an input node, got {:?}", surface.nodes.first());
    };
    assert_eq!(validation_error.as_deref(), Some("Name cannot be empty"));
    assert_eq!(
        accessibility.description.as_deref(),
        Some("Name cannot be empty"),
        "the rejection reason must outrank the usage hint while the value is rejected"
    );
    assert_eq!(accessibility.label, "Display name");
}
