// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `AppEngine` intercepts on the Contacts screen.
//!
//! `ContactListEngine` emits screen actions whose routing lives in
//! `AppEngine` (not the engine itself), so this file guards against
//! regressions where an action id on a Primary-styled button ends up
//! being a no-op because the intercept was removed or moved.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn new_vauchi_with_identity() -> Vauchi {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Test User").unwrap();
    vauchi
}

// @internal
#[test]
fn pressing_add_contact_on_contacts_navigates_to_exchange() {
    let vauchi = new_vauchi_with_identity();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);

    let before = engine.current_screen();
    assert_eq!(before.screen_id, "contact_list");

    // The Primary "Add Contact" button surfaces this action id.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "add_contact".into(),
    });

    match result {
        ActionResult::NavigateTo(ref screen) => {
            assert_eq!(
                screen.screen_id, "exchange_mode_selection",
                "add_contact must route to the Exchange mode-picker, got screen_id={}",
                screen.screen_id
            );
        }
        other => panic!(
            "pressing add_contact on Contacts must return NavigateTo(Exchange), got: {other:?}"
        ),
    }
}

// @internal
#[test]
fn pressing_go_exchange_on_contacts_navigates_to_exchange() {
    // Companion coverage for the `go_exchange` intercept, which is
    // emitted on the empty Contacts state and shares the same target
    // as `add_contact`. Kept as a sibling test so a refactor that
    // removes one intercept without the other is caught immediately.
    let vauchi = new_vauchi_with_identity();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "go_exchange".into(),
    });

    match result {
        ActionResult::NavigateTo(ref screen) => {
            assert_eq!(screen.screen_id, "exchange_mode_selection");
        }
        other => panic!("go_exchange must route to Exchange, got: {other:?}"),
    }
}
