// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `MoreEngine`.
//!
//! Single-screen menu (`more`) rendered as a `SectionedActionList`
//! (`more_menu`). Every menu entry is a `ListItemSelected`
//! pass-through (the handler accepts both `ActionPressed` and
//! `ListItemSelected` for the same ids); the screen carries no
//! standalone `ScreenAction`.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{MoreEngine, WorkflowEngine};

// @internal
#[test]
fn more_screen_is_reachable() {
    let engine = MoreEngine::new(vauchi_app::i18n::Locale::English);
    assert_eq!(engine.current_screen().screen_id, "more");
    assert_reachability(&engine, &[]);
}
