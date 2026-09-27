// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Onboarding completion must not drop a contact detail silently
//! (`2026-08-07-onboarding-contact-info-silently-dropped`, Goal 2).
//!
//! The ContactInfo step validates what the user types, so a value that
//! fails to save cannot arrive through the screens. The test pushes one
//! straight into the cached `OnboardingEngine` — the same
//! `OnboardingUpdate::PushField` seam the add-field form uses.

use super::{AppEngine, AppScreen};
use crate::ui::onboarding::FieldSetup;
use crate::ui::{EngineUpdate, OnboardingUpdate, UserAction, WorkflowEngine};
use vauchi_core::Command;
use vauchi_core::api::Vauchi;

fn press(engine: &mut AppEngine, action_id: &str) {
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    });
}

fn push_field(engine: &mut AppEngine, field_type: &str, label: &str, value: &str) {
    assert!(
        engine
            .engine
            .apply_update(EngineUpdate::Onboarding(OnboardingUpdate::PushField(
                FieldSetup {
                    field_type: field_type.into(),
                    label: label.into(),
                    value: value.into(),
                    visible_to_groups: Vec::new(),
                    shown: true,
                },
            ))),
        "the onboarding engine must accept PushField"
    );
}

/// Drives onboarding up to WhatNext with no quick-add fields, so the
/// only fields completion sees are the ones the test pushes.
fn engine_at_what_next() -> AppEngine {
    let mut engine = AppEngine::new(Vauchi::in_memory().unwrap());
    press(&mut engine, "create_new");
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    press(&mut engine, "continue");
    press(&mut engine, "continue");
    press(&mut engine, "continue");
    engine
}

// @internal
#[test]
fn onboarding_completion_reports_a_detail_that_failed_to_save() {
    let mut engine = engine_at_what_next();
    push_field(&mut engine, "email", "Email", "alice@example.com");
    push_field(&mut engine, "phone", "Phone", "not a phone number");

    press(&mut engine, "start_app");

    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::MyInfo,
        "a save shortfall must not block the user from reaching MyInfo"
    );
    let card = engine.vauchi().own_card().unwrap().unwrap();
    assert!(
        card.fields()
            .iter()
            .any(|f| f.value() == "alice@example.com"),
        "the detail that saved must still be on the card"
    );
    let alerts: Vec<_> = engine
        .drain_pending_commands()
        .into_iter()
        .filter_map(|command| match command {
            Command::PresentAlert { alert } => Some(alert),
            _ => None,
        })
        .collect();
    assert_eq!(alerts.len(), 1, "exactly one shortfall alert: {alerts:?}");
    assert_eq!(alerts[0].title, "Some details weren't saved");
    assert!(
        alerts[0].message.starts_with("1 of the details"),
        "the alert must count the unsaved details: {}",
        alerts[0].message
    );
}

// @internal
#[test]
fn onboarding_completion_without_a_shortfall_queues_no_alert() {
    let mut engine = engine_at_what_next();
    push_field(&mut engine, "email", "Email", "alice@example.com");

    press(&mut engine, "start_app");

    assert_eq!(*engine.current_app_screen(), AppScreen::MyInfo);
    assert!(
        !engine
            .drain_pending_commands()
            .iter()
            .any(|command| matches!(command, Command::PresentAlert { .. })),
        "a clean completion must not alert"
    );
}
