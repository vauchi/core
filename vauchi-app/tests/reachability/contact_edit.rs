// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `ContactEditEngine`.
//!
//! Three-step wizard with distinct screen_ids:
//! `edit_fields` -> `edit_visibility` -> `edit_preview`. The
//! `display_name` `TextInput` is primed by the structural walker, so
//! the non-empty gate on `edit_fields`' "continue"
//! (`contact_edit.rs:345`) clears and BFS traverses all three
//! screens. The reachable affordance set is `continue`
//! (fields + visibility), `back` (visibility + preview), and `save`
//! (preview).

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{ContactEditEngine, EditableContact, WorkflowEngine};

/// Action ids emitted across the three BFS-reachable screens and
/// consumed by `ContactEditEngine::handle_action` -
/// `core/vauchi-app/src/ui/contact_edit.rs`.
const HANDLED: &[&str] = &["continue", "back", "save"];

fn factory() -> ContactEditEngine {
    // Non-empty display name + no custom fields: the minimal valid
    // edit state. The walker also primes `display_name`, so the
    // "continue" gate clears regardless.
    ContactEditEngine::new(
        EditableContact {
            display_name: "Alice".into(),
            fields: Vec::new(),
        },
        Vec::new(),
    )
}

// @internal
#[test]
fn contact_edit_screens_are_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "edit_fields");
    assert_reachability_across_screens(factory, HANDLED);
}
