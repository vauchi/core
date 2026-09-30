// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `ContactEditEngine`.
//!
//! One form (`contact_edit`, #451) with no `ActionPressed` affordance:
//! Save and "Use their name" are body buttons (`ListItemSelected`
//! pass-throughs) and the name and note are `TextChanged` inputs. The
//! `discard_changes` / `keep_editing` confirmation appears only after a
//! Back with unsaved changes, which the static walker does not press;
//! `contact_edit_form_tests` covers it end to end.

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{ContactEditEngine, EditableContact, WorkflowEngine};

const HANDLED: &[&str] = &[];

fn factory() -> ContactEditEngine {
    ContactEditEngine::new(EditableContact {
        card_name: "Alice Liddell".into(),
        display_name: "Alice".into(),
        personal_note: String::new(),
    })
}

// @internal
#[test]
fn contact_edit_screens_are_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "contact_edit");
    assert_reachability_across_screens(factory, HANDLED);
}
