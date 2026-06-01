// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `AvatarEditorEngine`.
//!
//! The initial source-picker screen (`avatar_editor`) renders a
//! `sources` `ActionList` (camera / photos / generate / remove) plus
//! a `cancel` `ScreenAction`. The source rows are `ListItemSelected`
//! pass-throughs, so the only `ActionPressed` affordance is
//! `cancel`.
//!
//! NOTE (flagged for follow-up): the source rows are an `ActionList`
//! (so a tap emits `ListItemSelected`), but
//! `AvatarEditorEngine::handle_action` consumes `source_camera` /
//! `source_photos` / `source_generate` / `remove_avatar` only as
//! `ActionPressed`, with no `ListItemSelected { component_id:
//! "sources" }` arm. A structural walk therefore cannot advance past
//! the picker, and the downstream editing / generating screens stay
//! unreachable. Whether that is a real dispatch mismatch (taps no-op)
//! or a frontend that re-emits `ActionPressed` for these rows is
//! recorded in 2026-04-20-frontend-correctness-strategy as an L1
//! follow-up.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{AvatarEditorEngine, WorkflowEngine};

// @internal
#[test]
fn avatar_source_picker_is_reachable() {
    let engine = AvatarEditorEngine::new("Alice".into(), false);
    assert_eq!(engine.current_screen().screen_id, "avatar_editor");
    assert_reachability(&engine, &["cancel"]);
}
