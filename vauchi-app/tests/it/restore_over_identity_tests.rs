// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Restore from Settings on a device that already holds an identity
//! (vauchi/private#819): it used to offer "Replace Identity?" and then
//! always fail, because a backup restores only into an empty app.

use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;

const PASSWORD: &str = "correct horse battery staple";

fn backup_of(name: &str) -> Vec<u8> {
    let mut donor = Vauchi::in_memory().unwrap();
    donor.create_identity(name).unwrap();
    donor.export_full_backup(PASSWORD).unwrap().into_bytes()
}

fn title(result: &ActionResult) -> String {
    match result {
        ActionResult::NavigateTo(screen) | ActionResult::UpdateScreen(screen) => {
            screen.title.clone()
        }
        other => format!("{other:?}"),
    }
}

// @internal
#[test]
fn restore_from_settings_on_a_device_with_an_identity_explains_and_keeps_it() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let _ = engine.navigate_to(AppScreen::Backup);
    let explanation = get_string(Locale::ENGLISH, "backup.wizard.restore_needs_empty_title");

    let chosen = engine.handle_action(UserAction::ListItemSelected {
        component_id: "backup_rows".into(),
        item_id: "restore".into(),
    });
    let _ = engine.handle_hardware_event(Event::FilePickedFromUser {
        bytes: backup_of("Bob"),
        filename: "bob.vauchi".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_replace".into(),
    });

    assert_eq!(
        (
            title(&chosen),
            engine.current_screen().title,
            engine
                .vauchi()
                .identity()
                .map(|i| i.display_name().to_string()),
        ),
        (explanation.clone(), explanation, Some("Alice".to_string()))
    );
}
