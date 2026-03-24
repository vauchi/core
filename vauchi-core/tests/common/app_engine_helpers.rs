// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared helpers for AppEngine test files.

use vauchi_core::api::Vauchi;
use vauchi_core::ui::{ActionResult, AppEngine, UserAction, WorkflowEngine};

/// Drive through the full onboarding flow, returning the final ActionResult.
/// Each intermediate step is asserted to produce the expected ActionResult variant (T-12).
pub fn drive_onboarding(engine: &mut AppEngine) -> ActionResult {
    // Step 1: create_new -> navigates to welcome
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 1 (create_new) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "welcome",
        "create_new should navigate to welcome"
    );

    // Step 2: get_started -> navigates to default_name
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 2 (get_started) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "default_name",
        "get_started should navigate to default_name"
    );

    // Step 3: enter display name -> updates screen
    let r = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    let ActionResult::UpdateScreen(screen) = r else {
        panic!("Step 3 (TextChanged display_name) expected UpdateScreen, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "default_name",
        "TextChanged should update the default_name screen"
    );

    // Step 4: continue -> navigates to skip_gate
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 4 (continue) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "skip_gate",
        "continue should navigate to skip_gate"
    );

    // Step 5: skip_to_finish -> navigates to security_explanation
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip_to_finish".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 5 (skip_to_finish) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "security_explanation",
        "skip_to_finish should navigate to security_explanation"
    );

    // Step 6: continue -> navigates to backup_prompt
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 6 (continue) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "backup_prompt",
        "continue should navigate to backup_prompt"
    );

    // Step 7: skip -> navigates to ready
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 7 (skip) expected NavigateTo, got {r:?}");
    };
    assert_eq!(screen.screen_id, "ready", "skip should navigate to ready");

    // Step 8: start -> Complete -> AppEngine routes to Home
    engine.handle_action(UserAction::ActionPressed {
        action_id: "start".into(),
    })
}

/// Drive onboarding with "setup_backup" pressed (instead of skip) at the backup prompt.
/// Returns the final ActionResult which should navigate to the backup screen.
pub fn drive_onboarding_with_backup(engine: &mut AppEngine) -> ActionResult {
    // Steps 1-6: same as drive_onboarding (through backup_prompt)
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip_to_finish".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    // Step 7: setup_backup (instead of skip) -> navigates to ready
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "setup_backup".into(),
    });

    // Step 8: start -> Complete -> AppEngine routes to backup
    engine.handle_action(UserAction::ActionPressed {
        action_id: "start".into(),
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
        screen.screen_id, "welcome",
        "create_new should navigate to welcome"
    );
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("get_started should produce NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "default_name",
        "get_started should navigate to default_name"
    );
    // Attempt to continue without setting display_name
    engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    })
}

/// Helper: find a toggle's enabled state in a settings screen.
pub fn find_settings_toggle(
    screen: &vauchi_core::ui::ScreenModel,
    group_id: &str,
    item_id: &str,
) -> bool {
    use vauchi_core::ui::{Component, SettingsItemKind};
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
