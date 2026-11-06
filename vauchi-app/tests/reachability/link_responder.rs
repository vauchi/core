// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reachability test for `LinkResponderEngine`.
//!
//! Phase 1 T5 of `2026-04-27-deep-link-responder-flow`. The responder
//! starts on the code screen, whose Continue the app engine intercepts,
//! and then waits with one top-level action, Cancel, while the exchange
//! runs in the background. CC-22 requires the declared set match what the
//! walker emits.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{
    LINK_RESPONDER_ACTION_CANCEL, LINK_RESPONDER_ACTION_CONFIRM_CODE, LinkResponderEngine,
    WorkflowEngine,
};
use vauchi_core::exchange::link_mode::{initiator_generate, parse_exchange_deep_link};

/// Full handler set for `LinkResponderEngine` —
/// `core/vauchi-app/src/ui/link_responder.rs`.
const HANDLED: &[&str] = &[
    LINK_RESPONDER_ACTION_CANCEL,
    LINK_RESPONDER_ACTION_CONFIRM_CODE,
];

fn factory() -> LinkResponderEngine {
    let (init, _) = initiator_generate();
    let payload = parse_exchange_deep_link(&init.url).expect("canonical URL parses");
    LinkResponderEngine::new(payload)
}

// @internal
#[test]
fn link_responder_screen_is_fully_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "link_responder_code");
    assert_reachability_across_screens(factory, HANDLED);
}

// @internal
#[test]
fn link_responder_has_no_orphans() {
    let report = check_reachability(factory, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}
