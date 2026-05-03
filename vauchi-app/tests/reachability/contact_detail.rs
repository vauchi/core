// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `ContactDetailEngine`.
//!
//! Pair 3 of the Pure Humble UI retirement work
//! (`_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens/`).

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{ContactDetailEngine, Field, Item, WorkflowEngine};
use vauchi_core::contact::trust::TrustLevel;

/// Action ids handled by `ContactDetailEngine` —
/// `core/vauchi-app/src/ui/contact_detail.rs`. The `preview-as:<id>`
/// id is dynamic per contact; the factory below seeds id `c-alice` so
/// the walker observes the matching `preview-as:c-alice`.
///
/// `verify_fingerprint` is gated on `verify_button_visible(is_verified,
/// trust_level)` — the factory sets is_verified=false + Standard trust
/// so the action IS emitted.
///
/// `confirm_delete_contact` / `cancel_delete_contact` reach the same
/// screen_id via the InlineConfirm second-state ScreenModel which the
/// BFS walker dedupes (same shape as form_dialog reachability test).
const HANDLED: &[&str] = &[
    "edit",
    "verify_fingerprint",
    "preview-as:c-alice",
    "toggle_hidden",
    "delete_contact",
    "back",
];

fn factory() -> ContactDetailEngine {
    let item = Item {
        id: "c-alice".into(),
        name: "Alice".into(),
        subtitle: None,
        avatar_initials: "A".into(),
        status: None,
        searchable_fields: vec![],
        actions: vec![],
        a11y: None,
    };
    let fields: Vec<Field> = vec![];
    ContactDetailEngine::new(item, fields, String::new())
        .with_imported(true)
        .with_verification(false, TrustLevel::Standard)
        .with_fingerprint("ABCD-1234-EFGH-5678".into())
}

// @internal
#[test]
fn contact_detail_screen_is_fully_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "contact_detail");
    assert_reachability_across_screens(factory, HANDLED);
}

// @internal
#[test]
fn contact_detail_has_no_orphans() {
    let report = check_reachability(factory, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}
