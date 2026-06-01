// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `ContactLimitEngine`.
//!
//! Single-screen engine (`contact_limit`). In view mode the only
//! `ScreenAction` is `edit`; pressing it flips to edit mode (which
//! renders `save` / `cancel_edit`) but keeps
//! `screen_id == "contact_limit"`, so BFS `screen_id` dedup
//! collapses that variant. `save` / `cancel_edit` are covered by the
//! engine's inline tests; declaring them here would make them orphan
//! handlers.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{ContactLimitEngine, WorkflowEngine};

// @internal
#[test]
fn contact_limit_screen_is_reachable() {
    let engine = ContactLimitEngine::new(50, 100);
    assert_eq!(engine.current_screen().screen_id, "contact_limit");
    assert_reachability(&engine, &["edit"]);
}
