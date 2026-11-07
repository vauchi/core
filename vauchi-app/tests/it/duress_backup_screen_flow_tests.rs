// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Backup screens under duress export a decoy backup, never the real
//! identity (ADR-080). Acceptance test for ADR-080 step 3, ignored until the
//! duress foreground is built from the mask profile (vauchi/private#1117).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::{AuthMode, contact_card::ContactCard};

const BACKUP_PASSWORD: &str = "correct-horse-battery-staple-mask-9";

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn type_into(engine: &mut AppEngine, component_id: &str, value: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: component_id.into(),
        value: value.into(),
    });
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
#[ignore = "ADR-080 step 3, vauchi/private#1117"]
fn the_backup_screens_under_duress_export_a_backup_that_is_not_the_real_identity() {
    let mut vauchi = Vauchi::in_memory().expect("install");
    vauchi.create_identity("Real Owner").expect("identity");
    let real_public_id = vauchi.public_id().expect("id");
    vauchi
        .setup_app_password("normal-pin-1234")
        .expect("app password");
    vauchi.setup_duress_password("112233").expect("duress PIN");
    vauchi
        .add_decoy_contact("decoy", "Decoy", &ContactCard::new("Decoy"))
        .expect("decoy");
    assert_eq!(
        vauchi.authenticate("112233").expect("unlock"),
        AuthMode::Duress
    );
    let mut engine = AppEngine::new(vauchi);

    let _ = engine.navigate_to(AppScreen::Backup);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "backup_rows".into(),
        item_id: "create".into(),
    });
    type_into(&mut engine, "password", BACKUP_PASSWORD);
    let first = press(&mut engine, "continue");
    type_into(&mut engine, "confirm_password", BACKUP_PASSWORD);
    let second = press(&mut engine, "continue");
    let backup = [first, second]
        .into_iter()
        .find_map(|result| match result {
            ActionResult::BackupExportComplete { data } => Some(data),
            _ => None,
        })
        .expect("the duress backup flow completes");

    let mut restored = Vauchi::in_memory().expect("fresh install");
    restored
        .import_full_backup(&backup, BACKUP_PASSWORD)
        .expect("a duress backup restores");
    assert_ne!(restored.public_id().expect("id"), real_public_id);
}
