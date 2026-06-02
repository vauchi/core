// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `EmergencyBroadcastEngine`.
//!
//! Multi-screen flow with distinct screen_ids:
//! `emergency_overview` → {`emergency_contacts` → `emergency_message`}
//! (configure) and `emergency_confirm_send` (send). The factory builds a
//! *configured* engine so the overview exposes `send` / `disable` in
//! addition to `configure`.
//!
//! BFS reaches: the structural walker fills the `contact_ids` TextInput,
//! clearing the non-empty `continue` gate (`emergency_broadcast.rs`), so it
//! advances overview → contacts → message. Pressing `send` opens the
//! distinct-id confirm-send screen (`confirm_send` / `cancel_send`).
//!
//! Pinned elsewhere (not BFS-reachable): `confirm_disable` /
//! `cancel_disable` — pressing `disable` flips `pending_disable` and adds an
//! `InlineConfirm`, but that screen keeps `screen_id == "emergency_overview"`,
//! so `screen_id` dedup collapses it (same as `DuressPinEngine`). Covered
//! end-to-end by `core/vauchi-core/tests/it/emergency_broadcast_engine_tests.rs`.

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{EmergencyBroadcastEngine, WorkflowEngine};
use vauchi_core::types::EmergencyBroadcastConfig;

/// Action ids emitted across the BFS-reachable screens and consumed by
/// `EmergencyBroadcastEngine::handle_action` —
/// `core/vauchi-app/src/ui/emergency_broadcast.rs`.
const HANDLED: &[&str] = &[
    "configure",
    "send",
    "disable",
    "back",
    "continue",
    "save",
    "confirm_send",
    "cancel_send",
];

fn factory() -> EmergencyBroadcastEngine {
    EmergencyBroadcastEngine::new(Some(EmergencyBroadcastConfig {
        trusted_contact_ids: vec!["abc".into()],
        message: "help".into(),
        include_location: false,
    }))
}

// @internal
#[test]
fn emergency_broadcast_screens_are_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "emergency_overview");
    assert_reachability_across_screens(factory, HANDLED);
}
