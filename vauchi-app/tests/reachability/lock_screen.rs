// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `LockScreenEngine`.
//!
//! Single-screen PIN gate (`lock_screen`). The `pin` field is a
//! `PinInput` (not walker-primed) and `auth_failed` is a hardware
//! event, not a rendered affordance; the only `ScreenAction` is
//! `unlock`, consumed by `LockScreenEngine::handle_action`
//! (`core/vauchi-app/src/ui/lock_screen.rs`).

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{LockScreenEngine, WorkflowEngine};

// @internal
#[test]
fn lock_screen_is_reachable() {
    let engine = LockScreenEngine::new(3);
    assert_eq!(engine.current_screen().screen_id, "lock_screen");
    assert_reachability(&engine, &["unlock"]);
}
