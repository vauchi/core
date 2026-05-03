// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `GroupDetailEngine`.
//!
//! Pair 2 (LabelDetail) of the Pure Humble UI retirement work
//! (`_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens/`).

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{GroupDetailEngine, GroupFieldVisibility, Item, WorkflowEngine};

/// Action ids handled by `GroupDetailEngine` —
/// `core/vauchi-app/src/ui/group_detail.rs`. The `preview-as-member:<id>`
/// id is dynamic per member; the factory below seeds one member named
/// "alice" so the walker observes the matching `preview-as-member:c1`.
///
/// `confirm_delete_group` / `cancel_delete_group` are consumed by the
/// same handler but are only reachable from the `pending_delete`
/// `InlineConfirm` whose ScreenModel reuses `screen_id="group_detail"`
/// — the BFS dedupes on `screen_id` and skips the second variant.
/// Same shape as `form_dialog.rs`; tested manually elsewhere.
const HANDLED: &[&str] = &["rename", "delete_group", "preview-as-member:c1"];

fn factory() -> GroupDetailEngine {
    GroupDetailEngine::new(
        "g1".into(),
        "Work".into(),
        vec![Item {
            id: "c1".into(),
            name: "Alice".into(),
            subtitle: None,
            avatar_initials: "A".into(),
            status: None,
            searchable_fields: vec![],
            actions: vec![],
            a11y: None,
        }],
    )
    .with_field_visibility(vec![GroupFieldVisibility {
        field_id: "f1".into(),
        label: "Email".into(),
        value: "alice@example.com".into(),
        is_visible: true,
    }])
}

// @internal
#[test]
fn group_detail_screen_is_fully_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "group_detail");
    assert_reachability_across_screens(factory, HANDLED);
}

// @internal
#[test]
fn group_detail_has_no_orphans() {
    let report = check_reachability(factory, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}
