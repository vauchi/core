// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared helpers for AppEngine test files.

use vauchi_app::ui::{ActionResult, AppEngine, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

/// Drive through the full onboarding flow, returning the final ActionResult.
/// Each intermediate step is asserted to produce the expected ActionResult variant (T-12).
pub fn drive_onboarding(engine: &mut AppEngine) -> ActionResult {
    // Step 1: create_new -> navigates to default_name
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 1 (create_new) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "default_name",
        "create_new should navigate to default_name"
    );

    // Step 2: enter display name -> updates screen
    let r = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    let ActionResult::UpdateScreen(screen) = r else {
        panic!("Step 2 (TextChanged display_name) expected UpdateScreen, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "default_name",
        "TextChanged should update the default_name screen"
    );

    // Step 3: continue -> navigates to groups_setup
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 3 (continue) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "groups_setup",
        "continue should navigate to groups_setup"
    );

    // Step 4: continue -> navigates to contact_info
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 4 (continue) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "contact_info",
        "continue should navigate to contact_info"
    );

    // Step 5: continue -> navigates to what_next
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 5 (continue) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "what_next",
        "continue should navigate to what_next"
    );

    // Step 6: start_app -> CompleteWith -> AppEngine routes to Home
    engine.handle_action(UserAction::ActionPressed {
        action_id: "start_app".into(),
    })
}

/// Drive onboarding with "read_backup" pressed on the WhatNext screen.
/// Returns the final ActionResult which should navigate to the backup screen.
pub fn drive_onboarding_with_backup(engine: &mut AppEngine) -> ActionResult {
    // Steps 1-5: same as drive_onboarding (through what_next)
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Step 6: read_backup -> CompleteWith { BackupSetup } -> AppEngine routes to backup
    engine.handle_action(UserAction::ActionPressed {
        action_id: "read_backup".into(),
    })
}

/// Drive onboarding to the name step and attempt to continue without entering a name.
/// Returns the result of pressing "continue" without a display name.
pub fn drive_onboarding_without_name(engine: &mut AppEngine) -> ActionResult {
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("create_new should produce NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "default_name",
        "create_new should navigate to default_name"
    );
    // Attempt to continue without setting display_name
    engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    })
}

/// Helper: find a toggle's enabled state in a settings screen.
pub fn find_settings_toggle(
    screen: &vauchi_app::ui::ScreenModel,
    group_id: &str,
    item_id: &str,
) -> bool {
    use vauchi_app::ui::{Component, SettingsItemKind};
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::SettingsGroup { id, items, .. } if id == group_id => {
                items.iter().find_map(|item| match &item.kind {
                    SettingsItemKind::Toggle { enabled } if item.id == item_id => Some(*enabled),
                    _ => None,
                })
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Toggle '{item_id}' not found in group '{group_id}'"))
}

/// Helper: create an AppEngine with identity + password set, starting on Lock screen.
pub fn engine_with_password(password: &str) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password(password).unwrap();
    assert!(
        vauchi.is_password_enabled().unwrap(),
        "password should be enabled after setup"
    );
    AppEngine::new(vauchi)
}

/// Helper: enter a PIN into the lock screen engine.
pub fn enter_pin(engine: &mut AppEngine, pin: &str) {
    for ch in pin.chars() {
        // Intermediate step: accumulate PIN digits — unlock result asserted by caller
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "pin".into(),
            value: ch.to_string(),
        });
    }
}
