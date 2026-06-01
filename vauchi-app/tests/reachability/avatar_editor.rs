// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `AvatarEditorEngine`.
//!
//! The initial source-picker screen (`avatar_editor`) renders a
//! `sources` `ActionList` (camera / photos / generate [/ remove when an
//! avatar already exists]) plus a `cancel` `ScreenAction`. The source
//! rows are `ListItemSelected` pass-throughs, so the only
//! `ActionPressed` affordance is `cancel`.
//!
//! The source rows route through `handle_source_selection`
//! (`avatar_editor.rs`), which handles the `ListItemSelected
//! { component_id: "sources" }` shape every renderer emits for
//! `ActionList` rows (fixed 2026-06-01 — was a dispatch mismatch where
//! the handler matched these ids only as `ActionPressed`, so the rows
//! silently no-op'd on every platform; see
//! `2026-06-01-avatar-source-row-dispatch-mismatch`).
//!
//! Why this test still pins only `cancel` (not the downstream
//! `use` / `regenerate`): all three engine states — picker, editing,
//! generating — render under the **same** `screen_id` ("avatar_editor").
//! The across-screens BFS dedups by `screen_id`, so it cannot
//! distinguish the generating screen from the picker and never walks
//! its affordances. That is a property of this single-screen-id modal
//! engine, independent of the dispatch fix. The downstream
//! editing / generating handlers (`use`, `regenerate`, `save`, the
//! `gen_style` / `colors` `ListItemSelected` rows) and the source-row
//! dispatch itself are covered by `tests/it/avatar_editor_tests.rs`.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{AvatarEditorEngine, WorkflowEngine};

// @internal
#[test]
fn avatar_source_picker_is_reachable() {
    let engine = AvatarEditorEngine::new("Alice".into(), false);
    assert_eq!(engine.current_screen().screen_id, "avatar_editor");
    assert_reachability(&engine, &["cancel"]);
}
