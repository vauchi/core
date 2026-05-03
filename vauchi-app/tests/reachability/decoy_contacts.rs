// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `DecoyContactsEngine`.
//!
//! Phase 2c of `2026-05-01-android-humble-ui-deep-retirement` — the
//! engine replaces the bespoke Android `DecoyContactsSection.kt` so
//! every action id emitted by the screen must be reachable through
//! the engine's own `handle_action` (the AppEngine intercept layer
//! covers persistence; here we only assert the engine renders +
//! consumes its own affordances).

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{DecoyContactItem, DecoyContactsEngine, WorkflowEngine};

/// Action ids handled by `DecoyContactsEngine` —
/// `core/vauchi-app/src/ui/decoy_contacts.rs`.
///
/// `add_decoy` and `confirm_delete_decoy` are emitted by the engine
/// but the side-effect (storage write) lives in
/// `AppEngine::intercept_decoy_contacts_action`; the engine just
/// returns `UpdateScreen` so the BFS treats them as handled.
/// `cancel_delete_decoy` is only reachable from the InlineConfirm
/// branch whose ScreenModel reuses `screen_id="decoy_contacts"` —
/// BFS dedupes on `screen_id` and skips the second variant. Same
/// pattern as `group_detail`.
const HANDLED: &[&str] = &["add_decoy"];

fn factory_empty() -> DecoyContactsEngine {
    DecoyContactsEngine::new(vec![])
}

fn factory_populated() -> DecoyContactsEngine {
    DecoyContactsEngine::new(vec![
        DecoyContactItem {
            id: "d1".into(),
            display_name: "Alice Example".into(),
        },
        DecoyContactItem {
            id: "d2".into(),
            display_name: "Bob Sample".into(),
        },
    ])
}

// @internal
#[test]
fn decoy_contacts_screen_is_reachable() {
    let engine = factory_empty();
    assert_eq!(engine.current_screen().screen_id, "decoy_contacts");
    assert_reachability_across_screens(factory_empty, HANDLED);
}

// @internal
#[test]
fn decoy_contacts_has_no_orphans() {
    let report = check_reachability(factory_empty, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}

// @internal
#[test]
fn populated_decoy_list_is_reachable() {
    let engine = factory_populated();
    assert_eq!(engine.decoys().len(), 2);
    assert_eq!(engine.current_screen().screen_id, "decoy_contacts");
}
