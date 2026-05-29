// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Unit tests for the humble `LinkExchangeEngine` renderer (slice 32l
//! Phase 2). Mirrors the `LinkResponderEngine` test shape: each state's
//! `current_screen()` screen id + affordance set, and `handle_action`
//! outcomes for share / cancel / done / retry. The engine renders the
//! presentation state driven by the AppEngine link-initiator lifecycle;
//! it never owns the `LinkInitiatorSession`.

use vauchi_app::ui::{
    ActionResult, ActionStyle, Component, LINK_EXCHANGE_ACTION_CANCEL, LINK_EXCHANGE_ACTION_DONE,
    LINK_EXCHANGE_ACTION_RETRY, LINK_EXCHANGE_ACTION_SHARE, LinkExchangeEngine, UserAction,
    WorkflowEngine,
};
use vauchi_core::Command;

const URL: &str = "vauchi://exchange?v=1&epk=abc&hs=def&ps=ghi";

fn engine() -> LinkExchangeEngine {
    let mut e = LinkExchangeEngine::new();
    e.set_share_url(URL.to_string());
    e
}

fn press(id: &str) -> UserAction {
    UserAction::ActionPressed {
        action_id: id.to_string(),
    }
}

fn action_ids(engine: &LinkExchangeEngine) -> Vec<String> {
    engine
        .current_screen()
        .actions
        .iter()
        .map(|a| a.id.clone())
        .collect()
}

// @internal
#[test]
fn initial_state_renders_share_url_screen_with_share_and_cancel() {
    let engine = engine();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_share_url");
    assert_eq!(action_ids(&engine), vec!["share", "cancel"]);
    // The URL must reach the renderer for the share screen.
    let has_url = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::Text { content, .. } if content == URL));
    assert!(has_url, "share-url screen must render the shared URL");
}

// @internal
#[test]
fn share_action_emits_show_share_sheet_and_moves_to_waiting() {
    let mut engine = engine();
    let result = engine.handle_action(press(LINK_EXCHANGE_ACTION_SHARE));
    match result {
        ActionResult::Commands { commands } => {
            assert_eq!(commands.len(), 1);
            assert!(
                matches!(&commands[0], Command::ShowShareSheet { url } if url == URL),
                "share must emit ShowShareSheet with the link URL, got {:?}",
                commands[0]
            );
        }
        other => panic!("expected Commands, got {other:?}"),
    }
    assert_eq!(engine.current_screen().screen_id, "exchange_link_waiting");
    assert_eq!(action_ids(&engine), vec!["cancel"]);
}

// @internal
#[test]
fn cancel_on_share_url_screen_completes_and_marks_cancelled() {
    let mut engine = engine();
    let result = engine.handle_action(press(LINK_EXCHANGE_ACTION_CANCEL));
    assert!(matches!(result, ActionResult::Complete));
    assert!(engine.was_cancelled());
}

// @internal
#[test]
fn cancel_on_waiting_screen_completes_and_marks_cancelled() {
    let mut engine = engine();
    engine.transition_to_waiting();
    assert_eq!(engine.current_screen().screen_id, "exchange_link_waiting");
    let result = engine.handle_action(press(LINK_EXCHANGE_ACTION_CANCEL));
    assert!(matches!(result, ActionResult::Complete));
    assert!(engine.was_cancelled());
}

// @internal
#[test]
fn retrieving_screen_has_no_affordances() {
    let mut engine = engine();
    engine.transition_to_retrieving();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_link_retrieving");
    assert!(screen.actions.is_empty());
}

// @internal
#[test]
fn success_screen_offers_done_and_done_completes() {
    let mut engine = engine();
    engine.transition_to_success();
    assert_eq!(engine.current_screen().screen_id, "exchange_link_success");
    assert_eq!(action_ids(&engine), vec!["done"]);
    let result = engine.handle_action(press(LINK_EXCHANGE_ACTION_DONE));
    assert!(matches!(result, ActionResult::Complete));
    assert!(!engine.was_cancelled(), "Done on success is not a cancel");
}

// @internal
#[test]
fn failed_screen_offers_retry_and_cancel() {
    let mut engine = engine();
    engine.transition_to_failed("polling_timed_out".to_string());
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_link_failed");
    assert_eq!(action_ids(&engine), vec!["retry", "cancel"]);
    let retry_style = screen
        .actions
        .iter()
        .find(|a| a.id == "retry")
        .map(|a| a.style.clone());
    assert_eq!(retry_style, Some(ActionStyle::Primary));
}

// @internal
#[test]
fn retry_on_failed_screen_starts_a_fresh_link_exchange() {
    let mut engine = engine();
    engine.transition_to_failed("deposit_rejected".to_string());
    let result = engine.handle_action(press(LINK_EXCHANGE_ACTION_RETRY));
    assert!(matches!(result, ActionResult::StartLinkExchange));
}

// @internal
#[test]
fn cancel_on_failed_screen_completes() {
    let mut engine = engine();
    engine.transition_to_failed("decrypt_error".to_string());
    let result = engine.handle_action(press(LINK_EXCHANGE_ACTION_CANCEL));
    assert!(matches!(result, ActionResult::Complete));
    assert!(engine.was_cancelled());
}

// @internal
#[test]
fn transitions_are_inert_after_cancel() {
    let mut engine = engine();
    let _ = engine.handle_action(press(LINK_EXCHANGE_ACTION_CANCEL));
    assert!(engine.was_cancelled());
    // Lifecycle teardown may race a transition in; cancel-guarded setters
    // must not resurrect a terminal-rendered screen.
    engine.transition_to_success();
    assert_ne!(
        engine.current_screen().screen_id,
        "exchange_link_success",
        "a cancelled engine must not flip to success"
    );
}

// @internal
#[test]
fn first_terminal_transition_wins() {
    let mut engine = engine();
    engine.transition_to_success();
    engine.transition_to_failed("polling_timed_out".to_string());
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_link_success",
        "first terminal transition wins (mirrors LinkResponderEngine)"
    );
}

// @internal
#[test]
fn handle_hardware_event_returns_none() {
    let mut engine = engine();
    let out = engine.handle_hardware_event(vauchi_core::Event::LinkShared);
    assert!(out.is_none(), "renderer ignores hardware events");
}
