// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `EmergencyShredEngine`.
//!
//! One destructive screen (ADR-022, #419 item 4): `confirm_shred`
//! completes only when the `confirmation` TextInput holds the word WIPE,
//! handing the wipe to the AppEngine (#431); `cancel_shred` leaves.

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{EmergencyShredEngine, WorkflowEngine};

/// Action ids emitted by the one wipe screen and
/// consumed by `EmergencyShredEngine::handle_action` -
/// `core/vauchi-app/src/ui/emergency_shred.rs`.
const HANDLED: &[&str] = &["confirm_shred", "cancel_shred"];

fn factory() -> EmergencyShredEngine {
    EmergencyShredEngine::new(vauchi_app::i18n::Locale::English)
}

// @internal
#[test]
fn emergency_shred_screens_are_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "shred_warning");
    assert_reachability_across_screens(factory, HANDLED);
}
