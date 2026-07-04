// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `EmergencyBroadcastEngine` — the humble engine behind the
//! emergency-broadcast screen. Contact-id parsing and the trusted-contact
//! limit live in the engine (core), not the frontend (ADR-021 / ADR-043).

use vauchi_app::ui::*;
use vauchi_core::types::EmergencyBroadcastConfig;

fn configured() -> EmergencyBroadcastConfig {
    EmergencyBroadcastConfig {
        trusted_contact_ids: vec!["abc".into(), "def".into()],
        message: "help".into(),
        include_location: true,
    }
}

fn contact_item(id: &str, name: &str) -> Item {
    Item {
        id: id.into(),
        name: name.into(),
        subtitle: None,
        avatar_initials: name
            .chars()
            .next()
            .map(|c| c.to_string())
            .unwrap_or_default(),
        status: None,
        actions: vec![],
        a11y: None,
    }
}

/// A fresh engine advanced to the ContactIds step with the given pool available.
fn engine_at_contacts(available: Vec<Item>) -> EmergencyBroadcastEngine {
    let mut engine = EmergencyBroadcastEngine::new(None).with_available_contacts(available);
    engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });
    engine
}

// @internal
#[test]
fn overview_unconfigured_offers_only_configure() {
    let engine = EmergencyBroadcastEngine::new(None);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "emergency_overview");
    let ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec!["configure"], "unconfigured offers only configure");
}

// The overview status must be a read-only StatusIndicator, not an
// interactive ToggleList the engine snaps back (a control that cannot
// act — 2026-07-03-coercion-safety-config-gaps defect 3).
// @internal
#[test]
fn overview_shows_read_only_status_not_dead_toggle() {
    let engine = EmergencyBroadcastEngine::new(None);
    let screen = engine.current_screen();
    assert!(
        !screen.components.iter().any(|c| matches!(
            c,
            Component::ToggleList { id, .. } if id == "emergency_toggle"
        )),
        "overview must not render a dead interactive toggle for status"
    );
    let status = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::StatusIndicator { id, status, .. } if id == "emergency_status" => {
                Some(status.clone())
            }
            _ => None,
        })
        .expect("overview must show a read-only StatusIndicator");
    assert_eq!(
        status,
        Status::Warning,
        "unconfigured emergency broadcast must surface a Warning status"
    );
}

// @internal
#[test]
fn overview_configured_offers_send_and_disable() {
    let engine = EmergencyBroadcastEngine::new(Some(configured()));
    let screen = engine.current_screen();
    let ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
    assert!(ids.contains(&"configure"));
    assert!(ids.contains(&"send"));
    assert!(ids.contains(&"disable"));
}

// @internal
#[test]
fn configure_flow_saves_with_save_outcome() {
    let mut engine = engine_at_contacts(vec![
        contact_item("abc", "Abby"),
        contact_item("def", "Deb"),
    ]);

    // no recipient selected yet → validation error
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(
        matches!(&r, ActionResult::ValidationError { component_id, .. } if component_id == "contact_ids"),
        "empty contacts must fail, got {r:?}"
    );

    // pick both recipients → continue to message
    engine.handle_action(UserAction::ItemToggled {
        component_id: "contact_ids".into(),
        item_id: "abc".into(),
    });
    engine.handle_action(UserAction::ItemToggled {
        component_id: "contact_ids".into(),
        item_id: "def".into(),
    });
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(matches!(r, ActionResult::NavigateTo(ref s) if s.screen_id == "emergency_message"));

    // edit message, toggle location, save → Complete with Save outcome
    engine.handle_action(UserAction::TextChanged {
        component_id: "message".into(),
        value: "I need help".into(),
    });
    engine.handle_action(UserAction::ItemToggled {
        component_id: "options".into(),
        item_id: "include_location".into(),
    });
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });
    assert!(
        matches!(r, ActionResult::Complete),
        "save completes, got {r:?}"
    );
    assert_eq!(engine.outcome(), Some(&EmergencyOutcome::Save));
    assert_eq!(
        engine.contact_ids(),
        vec!["abc".to_string(), "def".to_string()]
    );
    assert_eq!(engine.message(), "I need help");
    assert!(engine.include_location());
}

// @internal
#[test]
fn too_many_contacts_is_rejected() {
    // 11 available contacts > MAX_TRUSTED_CONTACTS (10); select all of them.
    let available: Vec<Item> = (0..11)
        .map(|i| contact_item(&format!("c{i}"), &format!("C{i}")))
        .collect();
    let mut engine = engine_at_contacts(available.clone());
    for c in &available {
        engine.handle_action(UserAction::ItemToggled {
            component_id: "contact_ids".into(),
            item_id: c.id.clone(),
        });
    }
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(
        matches!(&r, ActionResult::ValidationError { component_id, .. } if component_id == "contact_ids"),
        "over-limit must fail, got {r:?}"
    );
}

// @internal
#[test]
fn send_flow_requires_confirmation_and_sets_send_outcome() {
    let mut engine = EmergencyBroadcastEngine::new(Some(configured()));

    // send → confirm screen (InlineConfirm)
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "send".into(),
    });
    assert!(
        matches!(r, ActionResult::NavigateTo(ref s) if s.screen_id == "emergency_confirm_send"),
        "send opens confirmation, got {r:?}"
    );

    // cancel returns to overview without sending
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_send".into(),
    });
    assert!(matches!(r, ActionResult::NavigateTo(ref s) if s.screen_id == "emergency_overview"));
    assert_eq!(engine.outcome(), None, "cancel must not set an outcome");

    // send again, confirm → Complete with Send outcome
    engine.handle_action(UserAction::ActionPressed {
        action_id: "send".into(),
    });
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_send".into(),
    });
    assert!(
        matches!(r, ActionResult::Complete),
        "confirm sends, got {r:?}"
    );
    assert_eq!(engine.outcome(), Some(&EmergencyOutcome::Send));
}

// @internal
#[test]
fn disable_requires_confirmation_and_sets_disable_outcome() {
    let mut engine = EmergencyBroadcastEngine::new(Some(configured()));

    // disable → InlineConfirm appears on overview
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "disable".into(),
    });
    assert!(matches!(r, ActionResult::UpdateScreen(_)));
    let has_confirm = engine
        .current_screen()
        .components
        .iter()
        .any(|c| matches!(c, Component::InlineConfirm { id, .. } if id == "disable"));
    assert!(has_confirm, "disable shows an InlineConfirm");

    // cancel clears it, no outcome
    engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_disable".into(),
    });
    assert_eq!(engine.outcome(), None);

    // disable + confirm → Complete with Disable outcome
    engine.handle_action(UserAction::ActionPressed {
        action_id: "disable".into(),
    });
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_disable".into(),
    });
    assert!(matches!(r, ActionResult::Complete));
    assert_eq!(engine.outcome(), Some(&EmergencyOutcome::Disable));
}

// @internal
#[test]
fn textinput_enter_submit_advances_like_continue_and_save() {
    // The message field keeps the keyboard Enter convention (`submit_<id>`);
    // the contacts step is now a picker advanced by `continue`.
    let mut engine = engine_at_contacts(vec![contact_item("abc", "Abby")]);
    engine.handle_action(UserAction::ItemToggled {
        component_id: "contact_ids".into(),
        item_id: "abc".into(),
    });
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(
        matches!(r, ActionResult::NavigateTo(ref s) if s.screen_id == "emergency_message"),
        "continue advances to message, got {r:?}"
    );
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit_message".into(),
    });
    assert!(
        matches!(r, ActionResult::Complete),
        "submit_message saves, got {r:?}"
    );
    assert_eq!(engine.outcome(), Some(&EmergencyOutcome::Save));
}

// The contacts step renders a picker over ALL contacts, marking the chosen
// ones — replacing the old hex-ID TextInput (config-gaps defect 2).
// @internal
#[test]
fn contacts_picker_renders_pool_and_reflects_selection() {
    let mut engine =
        engine_at_contacts(vec![contact_item("x", "Xander"), contact_item("y", "Yara")]);
    engine.handle_action(UserAction::ItemToggled {
        component_id: "contact_ids".into(),
        item_id: "x".into(),
    });

    let items = engine
        .current_screen()
        .components
        .into_iter()
        .find_map(|c| match c {
            Component::ToggleList { id, items, .. } if id == "contact_ids" => Some(items),
            _ => None,
        })
        .expect("a 'contact_ids' ToggleList");

    assert_eq!(items.len(), 2, "both contacts are offered");
    assert!(
        items.iter().any(|i| i.id == "x" && i.selected),
        "x shows as selected"
    );
    assert!(
        items.iter().any(|i| i.id == "y" && !i.selected),
        "y shows as unselected"
    );
}
