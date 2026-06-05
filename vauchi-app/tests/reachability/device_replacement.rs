// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `DeviceReplacementEngine`.
//!
//! Eight-screen wizard with three entry points
//! (`new_target` / `new_source` / `new_post_restore`). The middle
//! screens - `replacement_verify`, `replacement_syncing`,
//! `replacement_complete`, `replacement_decommission`,
//! `replacement_confirm_decommission` - are reached only across
//! hardware/transport boundaries: the QR-shown -> verify and
//! syncing -> complete hops fire on the `processing_complete`
//! callback (`device_replacement.rs:114,130`), not a UI action, so a
//! structural walk cannot drive them. Each entry screen is therefore
//! its own BFS island, and one test pins each.
//!
//! The hardware-gated middle screens (`confirm` / `reject` /
//! `decommission` / `done` / `remove_old` / `keep_both` /
//! `confirm_remove` / `cancel_remove`) are covered by
//! `core/vauchi-core/tests/it` device-replacement integration tests.
//! Declaring them here would make them orphan handlers.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{DeviceReplacementEngine, WorkflowEngine};

// @internal
#[test]
fn target_select_mode_screen_is_reachable() {
    // New (target) device: choose "I have my old device" /
    // "I lost my old device". Neither changes `step` (link / commands
    // / complete), so the select-mode screen is a single-screen island.
    // The footer `back` was dropped in the Goal 3 back-chrome sweep:
    // SelectMode is a pushed sub-screen (chrome `can_go_back` true), so
    // the footer button duplicated the core-driven back affordance —
    // and its `cancelled`→`Complete` routed to the same `navigate_back`.
    let engine = DeviceReplacementEngine::new_target();
    assert_eq!(engine.current_screen().screen_id, "replacement_select_mode");
    assert_reachability(&engine, &["has_old_device", "lost_device"]);
}

// @internal
#[test]
fn source_show_qr_screen_is_reachable() {
    // Old (source) device: shows the pairing QR. The only UI
    // affordance is `cancel`; the advance to verify happens on the
    // hardware `processing_complete` callback.
    let engine = DeviceReplacementEngine::new_source();
    assert_eq!(engine.current_screen().screen_id, "replacement_show_qr");
    assert_reachability(&engine, &["cancel"]);
}

// @internal
#[test]
fn post_restore_guidance_screen_is_reachable() {
    // Post-restore (old device lost): explains contact loss and
    // offers `social_recovery` / `done`.
    let engine = DeviceReplacementEngine::new_post_restore();
    assert_eq!(
        engine.current_screen().screen_id,
        "replacement_restore_guidance"
    );
    assert_reachability(&engine, &["social_recovery", "done"]);
}
