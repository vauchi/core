// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::{AppEngine, AppScreen, ReplacementRole};
use vauchi_core::{
    BindingId, Command, Event, ImportSource, InputMode, InputValue, MotionPreference,
    PresentationNode, SurfaceId, api::Vauchi, contact::Contact, contact_card::ContactCard,
};

fn app_with_contact() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory core");
    vauchi.create_identity("Alice").expect("identity");
    let contact = Contact::from_import(
        "contact-bob".into(),
        ContactCard::new("Bob"),
        ImportSource::VcardFile,
        None,
        0,
    );
    vauchi.add_contact(contact).expect("contact");
    let mut app = AppEngine::new(vauchi);
    app.set_initial_screen(AppScreen::Contacts);
    app
}

fn first_contact_interaction(
    commands: &[Command],
    expected_surface: &str,
) -> (SurfaceId, vauchi_core::InteractionId) {
    commands
        .iter()
        .find_map(|command| {
            let Command::ReplaceSurface { surface } = command else {
                return None;
            };
            if surface.surface_id.as_str() != expected_surface {
                return None;
            }
            surface.nodes.iter().find_map(|node| {
                let PresentationNode::List { rows, .. } = node else {
                    return None;
                };
                rows.first()
                    .and_then(|row| row.activation.as_ref())
                    .map(|action| (surface.surface_id.clone(), action.interaction_id.clone()))
            })
        })
        .expect("contact-row interaction")
}

fn first_toggle_binding(nodes: &[PresentationNode]) -> Option<BindingId> {
    nodes.iter().find_map(|node| match node {
        PresentationNode::Toggle { binding_id, .. } => Some(binding_id.clone()),
        PresentationNode::Group { children, .. } => first_toggle_binding(children),
        _ => None,
    })
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn initial_reducer_state_is_one_ordered_atomic_command_batch() {
    let mut app = AppEngine::new(Vauchi::in_memory().expect("in-memory core"));

    let commands = app.initial_commands().expect("initial commands");

    assert!(matches!(
        commands.first(),
        Some(Command::ReplaceSurface { surface }) if surface.revision == 1
    ));
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::SetContextBar { .. }))
    );
}

// @scenario: generic_presentation_protocol.feature :: Available window drives structural composition
#[test]
fn environment_event_reduces_to_core_owned_responsive_profile() {
    let mut app = AppEngine::new(Vauchi::in_memory().expect("in-memory core"));

    let commands = app
        .dispatch(Event::PresentationEnvironmentChanged {
            available_width: 700,
            available_height: 900,
            input_modes: vec![InputMode::Touch],
            motion: MotionPreference::Full,
        })
        .expect("environment event");

    assert!(matches!(
        commands.as_slice(),
        [Command::SetPresentationProfile { profile }]
            if profile.window_class == vauchi_core::WindowClass::Medium
    ));
}

// @scenario: generic_presentation_protocol.feature :: User interaction returns as an opaque event
#[test]
fn contextual_action_reduces_state_and_replaces_the_surface_at_the_next_revision() {
    let mut app = AppEngine::new(Vauchi::in_memory().expect("in-memory core"));
    let initial = app.initial_commands().expect("initial commands");
    let (surface_id, interaction_id) = initial
        .iter()
        .find_map(|command| {
            let Command::SetContextBar {
                surface_id, bar, ..
            } = command
            else {
                return None;
            };
            bar.primary
                .as_ref()
                .map(|primary| (surface_id.clone(), primary.interaction_id.clone()))
        })
        .expect("initial screen primary action");

    let commands = app
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id,
        })
        .expect("contextual action");

    assert!(matches!(
        commands.first(),
        Some(Command::ReplaceSurface { surface }) if surface.revision == 2
    ));
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn stale_body_interaction_is_rejected_after_replacement() {
    let mut app = AppEngine::new(Vauchi::in_memory().expect("in-memory core"));
    let _ = app.initial_commands().expect("initial commands");

    assert!(
        app.dispatch(Event::ActionActivated {
            surface_id: SurfaceId::new("onboarding").unwrap(),
            interaction_id: vauchi_core::InteractionId::new("surface.0.interaction.0").unwrap(),
        })
        .is_err()
    );
}

// @internal
#[test]
fn native_deep_link_reduces_to_core_owned_presentation_commands() {
    let mut app = AppEngine::new(Vauchi::in_memory().expect("in-memory core"));
    let _ = app.initial_commands().expect("initial commands");

    let commands = app
        .dispatch(Event::DeepLinkOpened {
            uri: "not-a-vauchi-link".to_owned(),
        })
        .expect("deep link event");

    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::PresentAlert { .. }))
    );
}

// @internal
#[test]
fn background_event_is_a_no_op_when_core_does_not_require_locking() {
    let mut app = AppEngine::new(Vauchi::in_memory().expect("in-memory core"));
    let _ = app.initial_commands().expect("initial commands");

    assert!(
        app.dispatch(Event::AppBackgrounded)
            .expect("background event")
            .is_empty()
    );
}

// @internal
#[test]
fn biometric_event_returns_only_a_generic_authentication_command() {
    let mut app = AppEngine::new(Vauchi::in_memory().expect("in-memory core"));

    let commands = app
        .dispatch(Event::BiometricUnlockSucceeded)
        .expect("biometric event");

    assert!(commands.iter().any(|command| matches!(
        command,
        Command::SetAuthenticationRequirement {
            requirement: vauchi_core::AuthenticationRequirement::Unlocked
        }
    )));
}

// @scenario: generic_presentation_protocol.feature :: Release contains only the generic action system
#[test]
fn device_replacement_entry_points_expose_only_the_generic_reducer_boundary() {
    for role in [
        ReplacementRole::Source,
        ReplacementRole::Target,
        ReplacementRole::PostRestore,
    ] {
        let mut app =
            AppEngine::for_device_replacement(Vauchi::in_memory().expect("in-memory core"), role);
        let commands = app
            .initial_commands()
            .expect("initial replacement commands");

        assert!(matches!(
            commands.first(),
            Some(Command::ReplaceSurface { surface }) if surface.revision == 1
        ));
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, Command::SetContextBar { .. }))
        );
    }
}

// @scenario: generic_presentation_protocol.feature :: Interaction activates its visible pane first
#[test]
fn selecting_a_contact_emits_a_live_primary_and_detail_surface_transaction() {
    let mut app = app_with_contact();
    let initial = app.initial_commands().expect("contacts transaction");
    let (surface_id, interaction_id) = first_contact_interaction(&initial, "contacts");

    app.dispatch(Event::PresentationEnvironmentChanged {
        available_width: 900,
        available_height: 700,
        input_modes: vec![InputMode::Pointer, InputMode::Keyboard],
        motion: MotionPreference::Full,
    })
    .expect("expanded environment");
    app.dispatch(Event::SurfaceActivated {
        surface_id: surface_id.clone(),
    })
    .expect("activate contacts");
    let commands = app
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id,
        })
        .expect("select contact");

    let surface_ids = commands
        .iter()
        .filter_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.surface_id.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(surface_ids, vec!["contacts", "contact_detail"]);
    assert!(commands.iter().any(|command| matches!(
        command,
        Command::SetPresentationProfile { profile }
            if profile.pane_layout == vauchi_core::PaneLayout::Split
                && profile.primary_surface.as_str() == "contacts"
                && profile.detail_surface.as_ref().map(SurfaceId::as_str) == Some("contact_detail")
                && profile.active_surface.as_str() == "contact_detail"
    )));

    let (primary_surface, primary_interaction) = first_contact_interaction(&commands, "contacts");
    let inactive_error = app
        .dispatch(Event::ActionActivated {
            surface_id: primary_surface.clone(),
            interaction_id: primary_interaction.clone(),
        })
        .expect_err("a retained pane must be activated before interaction");
    assert_eq!(
        inactive_error.to_string(),
        "invalid responsive presentation event: surface is not active"
    );
    app.dispatch(Event::SurfaceActivated {
        surface_id: primary_surface.clone(),
    })
    .expect("activate retained primary");
    let refreshed = app
        .dispatch(Event::ActionActivated {
            surface_id: primary_surface,
            interaction_id: primary_interaction,
        })
        .expect("interact with retained primary");
    assert_eq!(
        refreshed
            .iter()
            .filter(|command| matches!(command, Command::ReplaceSurface { .. }))
            .count(),
        2
    );
    assert_eq!(
        refreshed
            .iter()
            .filter(|command| matches!(command, Command::SetContextBar { .. }))
            .count(),
        2,
        "each retained surface owns contextual roles at the same revision"
    );
}

// @scenario: generic_presentation_protocol.feature :: Responsive transitions preserve interaction state
#[test]
fn changing_an_active_parent_pane_preserves_the_selected_detail_surface() {
    let mut app = app_with_contact();
    let initial = app.initial_commands().expect("contacts transaction");
    let (contacts_surface, contact_interaction) = first_contact_interaction(&initial, "contacts");
    app.dispatch(Event::PresentationEnvironmentChanged {
        available_width: 900,
        available_height: 700,
        input_modes: vec![InputMode::Pointer, InputMode::Keyboard],
        motion: MotionPreference::Full,
    })
    .expect("expanded environment");
    let split = app
        .dispatch(Event::ActionActivated {
            surface_id: contacts_surface.clone(),
            interaction_id: contact_interaction,
        })
        .expect("select contact");
    let toggle_binding = split
        .iter()
        .find_map(|command| {
            let Command::ReplaceSurface { surface } = command else {
                return None;
            };
            (surface.surface_id == contacts_surface)
                .then(|| first_toggle_binding(&surface.nodes))
                .flatten()
        })
        .expect("parent-pane toggle");

    app.dispatch(Event::SurfaceActivated {
        surface_id: contacts_surface.clone(),
    })
    .expect("activate retained parent");
    let recomposed = app
        .dispatch(Event::ValueChanged {
            surface_id: contacts_surface,
            binding_id: toggle_binding,
            value: InputValue::Boolean(true),
        })
        .expect("change parent-pane filter");

    assert_eq!(
        recomposed
            .iter()
            .filter_map(|command| match command {
                Command::ReplaceSurface { surface } => Some(surface.surface_id.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec!["contacts", "contact_detail"]
    );
    assert!(recomposed.iter().any(|command| matches!(
        command,
        Command::SetPresentationProfile { profile }
            if profile.pane_layout == vauchi_core::PaneLayout::Split
                && profile.active_surface.as_str() == "contacts"
    )));
}
