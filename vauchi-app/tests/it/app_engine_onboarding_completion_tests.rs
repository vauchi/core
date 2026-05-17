// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `AppEngine::handle_completion` —
//! specifically that the post-onboarding persistence path writes
//! the data the user collected during onboarding (display name,
//! selected groups, fields) to vauchi-core storage.
//!
//! Pinning these end-to-end assertions is the failing-test seam
//! for slice 32c (S2): closes the silent feature gap where
//! `OnboardingData.fields[]` are collected by `OnboardingEngine`
//! but never persisted to the user's own card on `Complete` /
//! `CompleteWith`.
//!
//! See: `_private/docs/problems/2026-05-17-slice-32c-mobile-ui-retirement/`.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

/// Send a `UserAction` and discard the returned `ActionResult`.
///
/// The driver tests below assert on observable state after the full
/// sequence (own card contents, identity presence, current screen),
/// not on intermediate `ActionResult` values, so swallowing the
/// return is intentional. `ActionResult` is `#[must_use]`, so without
/// this helper every call site would need `let _ =`.
fn act(engine: &mut AppEngine, action: UserAction) {
    let _ = engine.handle_action(action);
}

/// Drive the OnboardingEngine inside AppEngine from the initial
/// `IdentityCheck` screen all the way to the `WhatNext` →
/// `start_app` action that emits
/// `ActionResult::CompleteWith { MainScreen }`. This is the
/// production cold-start "create new identity with phone + email"
/// flow exactly as a frontend would drive it via
/// `handle_action_json`.
fn drive_full_onboarding_with_phone_and_email(engine: &mut AppEngine, display_name: &str) {
    // IdentityCheck → DefaultName ("create new identity")
    act(
        engine,
        UserAction::ActionPressed {
            action_id: "create_new".into(),
        },
    );

    // DefaultName: type display name, advance
    act(
        engine,
        UserAction::TextChanged {
            component_id: "display_name".into(),
            value: display_name.into(),
        },
    );
    act(
        engine,
        UserAction::ActionPressed {
            action_id: "continue".into(),
        },
    );

    // GroupsSetup: toggle "Family" on, then continue (selected_groups
    // path is already covered by routing.rs:296-298; included so
    // the test exercises the full pre-fields sequence).
    act(
        engine,
        UserAction::ItemToggled {
            component_id: "groups".into(),
            item_id: "Family".into(),
        },
    );
    act(
        engine,
        UserAction::ActionPressed {
            action_id: "continue".into(),
        },
    );

    // ContactInfo: reveal phone, type value, reveal email, type value,
    // continue. `sync_quick_add_fields` (onboarding.rs:855) pushes
    // both values into `OnboardingData.fields` on the "continue" press.
    act(
        engine,
        UserAction::ActionPressed {
            action_id: "show_phone".into(),
        },
    );
    act(
        engine,
        UserAction::TextChanged {
            component_id: "phone_input".into(),
            value: "+1-555-0100".into(),
        },
    );
    act(
        engine,
        UserAction::ActionPressed {
            action_id: "show_email".into(),
        },
    );
    act(
        engine,
        UserAction::TextChanged {
            component_id: "email_input".into(),
            value: "alice@example.com".into(),
        },
    );
    act(
        engine,
        UserAction::ActionPressed {
            action_id: "continue".into(),
        },
    );

    // WhatNext → start_app → CompleteWith { MainScreen }
    act(
        engine,
        UserAction::ActionPressed {
            action_id: "start_app".into(),
        },
    );
}

// @internal
#[test]
fn onboarding_completion_persists_phone_and_email_fields_to_own_card() {
    let vauchi = Vauchi::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::Onboarding,
        "no identity → cold start lands on Onboarding"
    );

    drive_full_onboarding_with_phone_and_email(&mut engine, "Alice");

    // After CompleteWith, identity exists and onboarding is complete.
    let v = engine.vauchi();
    assert!(v.has_identity(), "identity must be created on Complete");

    // The slice 32c assertion: fields collected during onboarding
    // (phone, email) must land on the user's own card. Today this
    // assertion FAILS because handle_completion (routing.rs:240)
    // does not walk `onboarding_data().fields[]` — only display_name
    // and selected_groups are persisted.
    let card = v
        .own_card()
        .expect("own_card lookup")
        .expect("own card present after identity creation");
    assert_eq!(
        card.display_name(),
        "Alice",
        "display name must round-trip on the card"
    );
    let phone_field = card
        .fields()
        .iter()
        .find(|f| f.value() == "+1-555-0100")
        .expect("phone field must be persisted to own_card after onboarding Complete");
    assert_eq!(
        phone_field.label(),
        "Phone",
        "phone field label must match what OnboardingEngine collected"
    );
    let email_field = card
        .fields()
        .iter()
        .find(|f| f.value() == "alice@example.com")
        .expect("email field must be persisted to own_card after onboarding Complete");
    assert_eq!(
        email_field.label(),
        "Email",
        "email field label must match what OnboardingEngine collected"
    );
}

// @internal
#[test]
fn onboarding_completion_with_no_fields_does_not_panic_and_creates_identity() {
    // CC-11 sibling: cover the empty-fields path so the soon-to-be-added
    // fields walk in handle_completion handles `Vec::is_empty()` cleanly
    // — no spurious error, identity still created.
    let vauchi = Vauchi::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    act(
        &mut engine,
        UserAction::ActionPressed {
            action_id: "create_new".into(),
        },
    );
    act(
        &mut engine,
        UserAction::TextChanged {
            component_id: "display_name".into(),
            value: "Bob".into(),
        },
    );
    act(
        &mut engine,
        UserAction::ActionPressed {
            action_id: "continue".into(),
        },
    );
    // Skip groups, skip contact info, jump to WhatNext via the skip
    // action chains.
    act(
        &mut engine,
        UserAction::ActionPressed {
            action_id: "continue".into(),
        },
    );
    act(
        &mut engine,
        UserAction::ActionPressed {
            action_id: "skip".into(),
        },
    );
    act(
        &mut engine,
        UserAction::ActionPressed {
            action_id: "start_app".into(),
        },
    );

    let v = engine.vauchi();
    assert!(v.has_identity(), "identity must be created");
    let card = v.own_card().expect("own_card lookup").expect("own card");
    assert!(
        card.fields().is_empty(),
        "no fields collected → own card has zero fields"
    );
}
