// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_app::ui::*;

// @internal
#[test]
fn gdpr_screen_id() {
    let engine = GdprEngine::new(
        None,
        "All consents granted".into(),
        vauchi_app::i18n::Locale::ENGLISH,
    );
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "privacy_settings");
}

// @internal
#[test]
fn gdpr_title() {
    let engine = GdprEngine::new(
        None,
        "All consents granted".into(),
        vauchi_app::i18n::Locale::ENGLISH,
    );
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Privacy & Data");
}

// @internal
#[test]
fn gdpr_shows_deletion_status() {
    let engine = GdprEngine::new(
        Some("Pending".into()),
        "All consents granted".into(),
        vauchi_app::i18n::Locale::ENGLISH,
    );
    let screen = engine.current_screen();

    let detail = find_info_detail(&screen, "privacy_info", "Deletion Status");
    assert_eq!(detail, "Pending");
}

// @internal
#[test]
fn gdpr_shows_no_deletion_requested_when_none() {
    let engine = GdprEngine::new(
        None,
        "All consents granted".into(),
        vauchi_app::i18n::Locale::ENGLISH,
    );
    let screen = engine.current_screen();

    let detail = find_info_detail(&screen, "privacy_info", "Deletion Status");
    assert_eq!(detail, "No deletion requested");
}

// @internal
#[test]
fn gdpr_export_without_an_app_password_asks_to_set_one() {
    let mut engine = GdprEngine::new(
        None,
        "All consents granted".into(),
        vauchi_app::i18n::Locale::ENGLISH,
    );
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "export".into(),
    });
    let ActionResult::NavigateTo(screen) = result else {
        panic!("export must not complete without verification, got {result:?}");
    };
    assert_eq!(screen.screen_id, "export_needs_app_password");
    assert_eq!(engine.engine_output(), None);
}

// @internal
#[test]
fn gdpr_export_engine_output_waits_on_the_verified_pin() {
    let mut engine = GdprEngine::new(
        None,
        "All consents granted".into(),
        vauchi_app::i18n::Locale::ENGLISH,
    )
    .with_app_password_set(true);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "export".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: "1234".into(),
    });
    assert_eq!(
        engine.engine_output(),
        Some(EngineOutput::ExportPin { pin: "1234".into() })
    );

    assert!(engine.apply_update(EngineUpdate::GdprExport(GdprExportUpdate::Verified)));
    assert_eq!(engine.current_screen().screen_id, "export_choose_format");
    assert_eq!(engine.engine_output(), None);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "export_json".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_readable_export".into(),
    });

    assert_eq!(result, ActionResult::Complete);
    assert_eq!(
        engine.engine_output(),
        Some(EngineOutput::Gdpr(GdprChoice::Export))
    );
}

// @internal
#[test]
fn gdpr_delete_navigates_to_confirmation() {
    let mut engine = GdprEngine::new(
        None,
        "All consents granted".into(),
        vauchi_app::i18n::Locale::ENGLISH,
    );
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(
                screen.screen_id, "delete_identity_summary",
                "Delete should navigate to confirmation screen"
            );
        }
        other => panic!("Expected NavigateTo, got {other:?}"),
    }
}

// @scenario: privacy_compliance :: With nobody trusted for recovery, deleting erases everything
#[test]
fn gdpr_confirm_delete_without_trusted_contacts_erases_everything() {
    let mut engine = GdprEngine::new(
        None,
        "All consents granted".into(),
        vauchi_app::i18n::Locale::ENGLISH,
    );
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_delete".into(),
    });
    assert_eq!(result, ActionResult::Complete);
    assert_eq!(
        engine.engine_output(),
        Some(EngineOutput::Gdpr(GdprChoice::Delete {
            erase_guardian_set: true,
            wipe_other_devices: false,
        }))
    );
}

// @internal
#[test]
fn gdpr_engine_output_initially_none() {
    let engine = GdprEngine::new(
        None,
        "All consents granted".into(),
        vauchi_app::i18n::Locale::ENGLISH,
    );
    assert_eq!(engine.engine_output(), None);
}

// @internal
#[test]
fn gdpr_unknown_action_returns_update_screen() {
    let mut engine = GdprEngine::new(
        None,
        "All consents granted".into(),
        vauchi_app::i18n::Locale::ENGLISH,
    );
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unknown".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "privacy_settings");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// --- helpers ---

fn find_info_detail(screen: &ScreenModel, panel_id: &str, item_title: &str) -> String {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::InfoPanel { id, items, .. } if id == panel_id => items
                .iter()
                .find(|item| item.title == item_title)
                .map(|item| item.detail.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("InfoItem '{item_title}' not found in panel '{panel_id}'"))
}
