// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `DeepLinkConsentEngine`.
//!
//! Phase 1 T3 of `2026-04-25-deeplink-consent-orchestrator`. Covers
//! the consent gate's state machine: starts pending, grant/deny
//! transitions, post-decision idempotency, unknown-action handling,
//! and the privacy invariant that the rendered `ScreenModel` does
//! not echo any payload bytes.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use vauchi_app::ui::{
    ActionResult, ActionStyle, ConsentDecision, DEEP_LINK_ACTION_DENY, DEEP_LINK_ACTION_GRANT,
    DeepLinkConsentEngine, UserAction, WorkflowEngine,
};
use vauchi_core::exchange::link_mode::{initiator_generate, parse_exchange_deep_link};

fn fresh_engine() -> DeepLinkConsentEngine {
    let (init, _) = initiator_generate();
    let payload = parse_exchange_deep_link(&init.url).expect("canonical URL parses");
    DeepLinkConsentEngine::new(payload)
}

// @internal
#[test]
fn starts_in_pending_with_grant_and_deny_actions() {
    let engine = fresh_engine();
    assert_eq!(engine.decision(), ConsentDecision::Pending);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "deep_link_consent");
    assert_eq!(screen.title, "Exchange Request");
    assert_eq!(screen.actions.len(), 2);
    assert_eq!(screen.actions[0].id, DEEP_LINK_ACTION_GRANT);
    assert_eq!(screen.actions[0].style, ActionStyle::Primary);
    assert_eq!(screen.actions[1].id, DEEP_LINK_ACTION_DENY);
    assert_eq!(screen.actions[1].style, ActionStyle::Secondary);
}

// @internal
#[test]
fn screen_does_not_leak_payload_bytes() {
    // Privacy: the consent prompt must never echo the parsed pk
    // or n bytes (or any base64 thereof) anywhere in the rendered
    // ScreenModel — neither title, subtitle, banner text, action
    // labels, nor any component content.
    let engine = fresh_engine();
    let pk_b64 = URL_SAFE_NO_PAD.encode(engine.payload().initiator_public_key());
    let nonce_b64 = URL_SAFE_NO_PAD.encode(engine.payload().nonce());
    let serialized = serde_json::to_string(&engine.current_screen()).expect("screen serializes");
    assert!(
        !serialized.contains(&pk_b64),
        "pk bytes leaked into ScreenModel"
    );
    assert!(
        !serialized.contains(&nonce_b64),
        "nonce bytes leaked into ScreenModel"
    );
}

// @internal
#[test]
fn grant_action_completes_with_granted_decision() {
    let mut engine = fresh_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: DEEP_LINK_ACTION_GRANT.into(),
    });
    assert_eq!(result, ActionResult::Complete);
    assert_eq!(engine.decision(), ConsentDecision::Granted);
    assert!(!engine.was_cancelled(), "grant is not a cancel");
}

// @internal
#[test]
fn deny_action_completes_with_denied_decision_and_cancelled_flag() {
    let mut engine = fresh_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: DEEP_LINK_ACTION_DENY.into(),
    });
    assert_eq!(result, ActionResult::Complete);
    assert_eq!(engine.decision(), ConsentDecision::Denied);
    assert!(
        engine.was_cancelled(),
        "deny must set cancelled so AppEngine skips persistence"
    );
}

// @internal
#[test]
fn unknown_action_id_returns_screen_unchanged() {
    let mut engine = fresh_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unknown".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "deep_link_consent");
        }
        other => panic!("expected UpdateScreen, got {other:?}"),
    }
    assert_eq!(engine.decision(), ConsentDecision::Pending);
}

// @internal
#[test]
fn second_grant_after_first_decision_is_inert() {
    // Race-safety: a fast double-tap must not transition state twice.
    let mut engine = fresh_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: DEEP_LINK_ACTION_GRANT.into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: DEEP_LINK_ACTION_GRANT.into(),
    });
    match result {
        ActionResult::UpdateScreen(_) => {}
        other => panic!("expected UpdateScreen on post-decision action, got {other:?}"),
    }
    assert_eq!(engine.decision(), ConsentDecision::Granted);
}

// @internal
#[test]
fn deny_after_grant_is_inert() {
    // Once the user grants, a subsequent deny must not flip the
    // decision (the responder flow may already be running).
    let mut engine = fresh_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: DEEP_LINK_ACTION_GRANT.into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: DEEP_LINK_ACTION_DENY.into(),
    });
    assert_eq!(engine.decision(), ConsentDecision::Granted);
    assert!(!engine.was_cancelled());
}

// @internal
#[test]
fn text_changed_action_is_inert() {
    // No text inputs on the consent screen — TextChanged must not
    // crash or flip decision.
    let mut engine = fresh_engine();
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "anything".into(),
        value: "anything".into(),
    });
    match result {
        ActionResult::UpdateScreen(_) => {}
        other => panic!("expected UpdateScreen, got {other:?}"),
    }
    assert_eq!(engine.decision(), ConsentDecision::Pending);
}

// @internal
#[test]
fn payload_round_trips_through_engine() {
    let (init, _) = initiator_generate();
    let payload = parse_exchange_deep_link(&init.url).unwrap();
    let engine = DeepLinkConsentEngine::new(payload);
    assert_eq!(*engine.payload().nonce(), init.nonce);
}
