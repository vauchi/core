// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine-level regression for the exchange Cancel white-screen.
//!
//! Fix A of `2026-06-02-exchange-back-cancel-broken`: Cancel on the
//! core-driven `MultiStageExchange` screen returns `ActionResult::Complete`,
//! which `AppEngine::handle_completion` had no arm for → the catch-all
//! `navigate_back` produced an empty `screen_id` → the frontend rendered
//! a white screen (device-confirmed, Pixel 3a). This pins that Cancel
//! lands on a real, non-empty screen (the mode picker) and Done lands on
//! Contacts.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// The frontend renders the screen carried by the action's result;
/// extract its `screen_id`.
fn navigate_screen_id(result: &ActionResult) -> &str {
    match result {
        ActionResult::NavigateTo(screen) => screen.screen_id.as_str(),
        other => panic!("expected NavigateTo from completion, got {other:?}"),
    }
}

// @internal
#[test]
fn cancel_on_multi_stage_exchange_lands_on_real_screen() {
    let mut engine = engine_with_identity();

    // Enter the exchange flow and pick Glance — core hands off to the
    // dedicated `MultiStageExchange` screen (no groups → direct handoff).
    let entry = engine.navigate_to(AppScreen::Exchange);
    assert_eq!(
        entry.screen_id, "exchange",
        "Exchange entry should show the mode picker under the canonical \
         tab-root id (`exchange`), so frontends render the bottom nav bar"
    );

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:glance".into(),
    });
    assert!(
        matches!(
            engine.current_app_screen(),
            AppScreen::MultiStageExchange { .. }
        ),
        "picking Glance should navigate to MultiStageExchange, got {:?}",
        engine.current_app_screen()
    );

    // Cancel the multi-stage exchange.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    let landed = navigate_screen_id(&result);

    // The white-screen bug: this `screen_id` was empty.
    assert!(
        !landed.is_empty(),
        "Cancel must not land on an empty screen_id (white-screen regression)"
    );
    assert_ne!(
        landed, "multi_stage_exchange",
        "Cancel must leave the multi-stage screen"
    );
    // The AppEngine must have actually navigated off the screen.
    assert!(
        !matches!(
            engine.current_app_screen(),
            AppScreen::MultiStageExchange { .. }
        ),
        "Cancel must navigate off the MultiStageExchange AppScreen, still on {:?}",
        engine.current_app_screen()
    );
    // Cancel returns to the mode picker so the user can retry — under the
    // canonical `exchange` tab-root id so the nav bar shows (no post-cancel
    // dead-end).
    assert_eq!(
        landed, "exchange",
        "Cancel should return to the mode picker (canonical tab-root id)"
    );
}

// Done (vs Cancel) on the multi-stage screen also returns `Complete`,
// but — the contact having already been persisted on the `Finalized`
// event — it must land on Contacts, not the picker. Covers the
// `was_cancelled() == false` branch of the completion arm.
// @internal
#[test]
fn done_on_multi_stage_exchange_lands_on_contacts() {
    let mut engine = engine_with_identity();

    let _ = engine.navigate_to(AppScreen::Exchange);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:glance".into(),
    });
    assert!(matches!(
        engine.current_app_screen(),
        AppScreen::MultiStageExchange { .. }
    ));

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "done".into(),
    });

    assert_eq!(
        navigate_screen_id(&result),
        "contacts",
        "Done must land on the contacts list (the new contact)"
    );
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::Contacts,
        "Done must navigate to the Contacts AppScreen"
    );
}
