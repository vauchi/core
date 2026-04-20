// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `FormDialogEngine`.
//!
//! Single-screen variant (not BFS). `FormDialogEngine` renders a
//! dialog whose two states (clean vs. `pending_discard`) share
//! the same `screen_id` — the BFS dedupes on `screen_id` and
//! would therefore miss the `confirm_discard` / `cancel_discard`
//! affordances the `InlineConfirm` injects when `pending_discard`
//! becomes true. Testing the clean initial state via the static
//! diff is the faithful coverage we can offer today.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{FormDialogEngine, FormDialogType, WorkflowEngine};

/// Action ids consumed on the clean (non-discard) `CreateGroup`
/// form — `core/vauchi-app/src/ui/form_dialog.rs:567-594`.
///
/// `confirm_discard` / `cancel_discard` are consumed by the same
/// handler but are only reachable from the dirty-form
/// `InlineConfirm`; tested manually elsewhere.
const CREATE_GROUP_CLEAN_HANDLED: &[&str] = &["submit", "cancel"];

// @internal
#[test]
fn create_group_initial_screen_is_reachable() {
    let engine = FormDialogEngine::new(FormDialogType::CreateGroup);
    assert_eq!(engine.current_screen().screen_id, "form_create_group");
    assert_reachability(&engine, CREATE_GROUP_CLEAN_HANDLED);
}
