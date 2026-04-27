// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `DeepLinkConsentEngine`.
//!
//! Phase 1 T5 of `2026-04-25-deeplink-consent-orchestrator`. The
//! consent gate exposes exactly two top-level actions; CC-22
//! requires the declared set match what the walker emits.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{
    DEEP_LINK_ACTION_DENY, DEEP_LINK_ACTION_GRANT, DeepLinkConsentEngine, WorkflowEngine,
};
use vauchi_core::exchange::link_mode::{initiator_generate, parse_exchange_deep_link};

/// Full handler set for `DeepLinkConsentEngine` —
/// `core/vauchi-app/src/ui/deep_link_consent.rs`.
const HANDLED: &[&str] = &[DEEP_LINK_ACTION_GRANT, DEEP_LINK_ACTION_DENY];

fn factory() -> DeepLinkConsentEngine {
    let (init, _) = initiator_generate();
    let payload = parse_exchange_deep_link(&init.url).expect("canonical URL parses");
    DeepLinkConsentEngine::new(payload)
}

// @internal
#[test]
fn deep_link_consent_screen_is_fully_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "deep_link_consent");
    assert_reachability_across_screens(factory, HANDLED);
}

// @internal
#[test]
fn deep_link_consent_has_no_orphans() {
    let report = check_reachability(factory, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}
