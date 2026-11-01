// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reachability test for `EmergencyBroadcastEngine` (ADR-036, private#895).
//!
//! The configured screen offers Send, Save and Disable as `ButtonList`
//! buttons, which a shell reports as `ListItemSelected` (not diffed
//! here, like the tag-promotion toggles). Send and Disable arm an
//! `InlineConfirm` whose `ActionPressed` ids live on a screen with the
//! same `screen_id`, which the BFS dedupes (the decoy_contacts pattern),
//! so no `ActionPressed` id is declared.

use vauchi_app::i18n::Locale;
use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{EmergencyBroadcastEngine, TrustedContactChoice, WorkflowEngine};

const HANDLED: &[&str] = &[];

fn factory() -> EmergencyBroadcastEngine {
    EmergencyBroadcastEngine::new(
        vec![
            TrustedContactChoice {
                id: "c1".into(),
                display_name: "Bob".into(),
                selected: true,
            },
            TrustedContactChoice {
                id: "c2".into(),
                display_name: "Carol".into(),
                selected: false,
            },
        ],
        "I may be in danger. Please check on me.".into(),
        false,
        true,
        Locale::ENGLISH,
    )
}

// @internal
#[test]
fn emergency_broadcast_screen_is_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "emergency_broadcast");
    assert_reachability_across_screens(factory, HANDLED);
}

// @internal
#[test]
fn emergency_broadcast_screen_has_no_orphans() {
    let report = check_reachability(factory, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}
