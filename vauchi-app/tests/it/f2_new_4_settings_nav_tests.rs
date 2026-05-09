// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! F2-NEW-4 dive: Settings → Duress PIN and → Create Backup wrongly navigate
//! to My Card on Pixel; Decoy Contacts (same SettingsItem.Link shape, same
//! intercept arm) works. The Kotlin → core action chain is fully traced
//! in the 2026-05-09 device-test-campaign-phase2 doc; this test exercises
//! the Rust side end-to-end so the bug (or its absence) is reproducible
//! without device flakiness.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_on_settings() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Settings);
    assert_eq!(*engine.current_app_screen(), AppScreen::Settings);
    engine
}

fn settings_link_action(item_id: &str) -> UserAction {
    UserAction::ListItemSelected {
        component_id: "security".into(),
        item_id: item_id.into(),
    }
}

// @internal
#[test]
fn duress_pin_link_navigates_to_duress_pin_screen() {
    let mut engine = engine_on_settings();
    let result = engine.handle_action(settings_link_action("duress_pin"));
    let landed = engine.current_app_screen().clone();
    match result {
        ActionResult::NavigateTo(_) => {
            assert_eq!(
                landed,
                AppScreen::DuressPin,
                "duress_pin link should land on DuressPin screen, got {landed:?}"
            );
        }
        other => {
            panic!("expected NavigateTo(DuressPin), got {other:?} (engine landed on {landed:?})")
        }
    }
}

// @internal
#[test]
fn decoy_contacts_link_navigates_to_decoy_contacts_screen() {
    let mut engine = engine_on_settings();
    let result = engine.handle_action(settings_link_action("decoy_contacts"));
    let landed = engine.current_app_screen().clone();
    match result {
        ActionResult::NavigateTo(_) => {
            assert_eq!(
                landed,
                AppScreen::DecoyContacts,
                "decoy_contacts link should land on DecoyContacts screen, got {landed:?}"
            );
        }
        other => panic!(
            "expected NavigateTo(DecoyContacts), got {other:?} (engine landed on {landed:?})"
        ),
    }
}

// @internal
#[test]
fn backup_export_link_navigates_to_backup_screen() {
    let mut engine = engine_on_settings();
    // Use the actual settings group component id for backup
    let action = UserAction::ListItemSelected {
        component_id: "backup".into(),
        item_id: "backup_export".into(),
    };
    let result = engine.handle_action(action);
    let landed = engine.current_app_screen().clone();
    match result {
        ActionResult::NavigateTo(_) => {
            assert_eq!(
                landed,
                AppScreen::Backup,
                "backup_export link should land on Backup screen, got {landed:?}"
            );
        }
        other => panic!("expected NavigateTo(Backup), got {other:?} (engine landed on {landed:?})"),
    }
}
