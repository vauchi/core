// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::{
    A11y, ActionStyle, ContextualSurface, ContextualSurfaceRoute, ScreenAction, ScreenModel,
    TabInfo, UserAction,
};
use vauchi_core::{
    ActionSpec, ActionTone, Command, Event, InteractionId, OverlayKind, StandardShortcut, SurfaceId,
};

fn screen_action(id: &str, label: &str, style: ActionStyle) -> ScreenAction {
    ScreenAction {
        id: id.to_owned(),
        label: label.to_owned(),
        style,
        enabled: true,
        a11y: Some(A11y::labeled(format!("{label} accessible"))),
    }
}

fn action(
    id: &str,
    label: &str,
    accessibility_label: &str,
    tone: ActionTone,
    shortcut: Option<StandardShortcut>,
) -> ActionSpec {
    ActionSpec {
        interaction_id: InteractionId::new(id).expect("valid interaction id"),
        label: label.to_owned(),
        accessibility_label: accessibility_label.to_owned(),
        icon_token: None,
        enabled: true,
        tone,
        shortcut,
    }
}

fn surface() -> SurfaceId {
    SurfaceId::new("contacts").expect("valid surface id")
}

fn contextual_surface() -> ContextualSurface {
    let mut screen = ScreenModel::new(
        "contacts",
        "Contacts",
        vec![],
        vec![
            screen_action("save", "Save", ActionStyle::Primary),
            screen_action("share", "Share", ActionStyle::Secondary),
            screen_action("delete", "Delete", ActionStyle::Destructive),
        ],
    );
    screen.nav_actions = vec![
        screen_action("go_back", "Back", ActionStyle::Secondary),
        screen_action("open_settings", "Settings", ActionStyle::Secondary),
    ];
    let navigation = vec![
        TabInfo {
            id: "contacts".into(),
            action_id: "contacts".into(),
            label: "Contacts".into(),
            icon: "person.2".into(),
            badge_count: 0,
        },
        TabInfo {
            id: "groups".into(),
            action_id: "groups".into(),
            label: "Groups".into(),
            icon: "folder".into(),
            badge_count: 2,
        },
    ];

    ContextualSurface::compose(surface(), &screen, &navigation, "Navigate", "More actions")
        .expect("valid contextual surface")
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Core supplies the four contextual roles
// @scenario: generic_presentation_protocol.feature :: Contextual controls expose four stable roles
#[test]
fn test_screen_actions_are_partitioned_into_four_core_owned_roles() {
    let surface = contextual_surface();
    let bar = surface.context_bar();

    assert_eq!(
        bar.back,
        Some(action(
            "presentation.back",
            "Back",
            "Back accessible",
            ActionTone::Standard,
            Some(StandardShortcut::Back),
        ))
    );
    assert_eq!(
        bar.navigation,
        Some(action(
            "presentation.navigation",
            "Navigate",
            "Navigate",
            ActionTone::Standard,
            None,
        ))
    );
    assert_eq!(
        bar.primary,
        Some(action(
            "save",
            "Save",
            "Save accessible",
            ActionTone::Standard,
            Some(StandardShortcut::ActivatePrimary),
        ))
    );
    assert_eq!(
        bar.secondary,
        Some(action(
            "presentation.secondary",
            "More actions",
            "More actions",
            ActionTone::Standard,
            None,
        ))
    );
    assert_eq!(
        surface.initial_commands(),
        vec![Command::SetContextBar {
            surface_id: self::surface(),
            revision: 0,
            bar: Box::new(bar.clone()),
        }]
    );

    let secondary = surface
        .handle_event(Event::ActionActivated {
            surface_id: self::surface(),
            interaction_id: InteractionId::new("presentation.secondary").unwrap(),
        })
        .expect("secondary launcher");
    let ContextualSurfaceRoute::Commands(commands) = secondary else {
        panic!("secondary launcher must emit overlay commands");
    };
    let Command::PresentOverlay {
        surface_id,
        overlay,
        revision,
    } = &commands[0]
    else {
        panic!("secondary launcher must present an overlay");
    };
    assert_eq!(surface_id.as_str(), "contacts");
    assert_eq!(*revision, 0);
    assert_eq!(overlay.kind, OverlayKind::ActionMenu);
    assert_eq!(
        overlay
            .items
            .iter()
            .map(|item| (item.interaction_id.as_str(), item.tone))
            .collect::<Vec<_>>(),
        vec![
            ("share", ActionTone::Standard),
            ("delete", ActionTone::Destructive),
            ("open_settings", ActionTone::Standard),
        ]
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Shells forward opaque interactions
// @scenario: generic_presentation_protocol.feature :: User interaction returns as an opaque event
#[test]
fn test_launchers_and_items_route_without_shell_domain_vocabulary() {
    let surface = contextual_surface();

    let navigation = surface
        .handle_event(Event::ActionActivated {
            surface_id: self::surface(),
            interaction_id: InteractionId::new("presentation.navigation").unwrap(),
        })
        .expect("navigation launcher");
    let ContextualSurfaceRoute::Commands(commands) = navigation else {
        panic!("navigation launcher must emit overlay commands");
    };
    assert!(matches!(
        &commands[0],
        Command::PresentOverlay { overlay, .. } if overlay.kind == OverlayKind::Navigation
    ));

    assert_eq!(
        surface
            .handle_event(Event::ActionActivated {
                surface_id: self::surface(),
                interaction_id: InteractionId::new("presentation.navigation.groups").unwrap(),
            })
            .expect("navigation item"),
        ContextualSurfaceRoute::UserAction(UserAction::NavigateToTab {
            action_id: "groups".into(),
        })
    );
    assert_eq!(
        surface
            .handle_event(Event::ActionActivated {
                surface_id: self::surface(),
                interaction_id: InteractionId::new("delete").unwrap(),
            })
            .expect("screen action"),
        ContextualSurfaceRoute::UserAction(UserAction::ActionPressed {
            action_id: "delete".into(),
        })
    );
    assert_eq!(
        surface
            .handle_event(Event::BackRequested {
                surface_id: self::surface(),
            })
            .expect("back request"),
        ContextualSurfaceRoute::UserAction(UserAction::NavigateBack)
    );
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn revisioned_context_rejects_an_activation_from_an_old_render() {
    let screen = ScreenModel::new(
        "contacts",
        "Contacts",
        vec![],
        vec![screen_action("save", "Save", ActionStyle::Primary)],
    );
    let current =
        ContextualSurface::compose_revisioned(surface(), 7, &screen, &[], "Navigate", "More")
            .expect("revisioned context");
    let current_primary = current
        .context_bar()
        .primary
        .as_ref()
        .expect("primary")
        .interaction_id
        .as_str();

    assert!(current_primary.starts_with("surface.7.context."));
    assert!(
        current
            .handle_event(Event::ActionActivated {
                surface_id: surface(),
                interaction_id: InteractionId::new("surface.6.context.save").unwrap(),
            })
            .is_err()
    );
}
