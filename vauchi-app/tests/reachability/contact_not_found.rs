// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `ContactNotFoundEngine`.
//!
//! Single-screen error state (`contact_not_found`) shown when a
//! contact id no longer resolves; the only affordance is `back`,
//! consumed by `ContactNotFoundEngine::handle_action`
//! (`core/vauchi-app/src/ui/contact_detail.rs`).

use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{ContactNotFoundEngine, WorkflowEngine};

// @internal
#[test]
fn contact_not_found_screen_is_reachable() {
    let engine = ContactNotFoundEngine::new("missing-contact".into());
    assert_eq!(engine.current_screen().screen_id, "contact_not_found");
    assert_reachability(&engine, &["back"]);
}
