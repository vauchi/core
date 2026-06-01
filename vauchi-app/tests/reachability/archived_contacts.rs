// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `ArchivedContactsEngine`.
//!
//! Single-screen read-only list (`archived_contacts`): an
//! `ActionList` of archived contacts (or an empty-state panel) with
//! no `ScreenAction`s. Rows are `ListItemSelected` pass-throughs.

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{ArchivedContactsEngine, WorkflowEngine};

// @internal
#[test]
fn archived_contacts_screen_is_reachable() {
    let engine = ArchivedContactsEngine::new(Vec::new());
    assert_eq!(engine.current_screen().screen_id, "archived_contacts");
    assert_reachability(&engine, &[]);
}
