// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! On a new device, entering the lost identity's key starts a recovery
//! the device remembers, so vouchers have a claim to attach to.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::{Identity, Vauchi};

// @scenario: contact_recovery :: Remember old identity fingerprint
#[test]
fn creating_a_claim_for_the_old_key_stores_the_recovery_for_that_key() {
    let lost = Identity::create("Alice", 0);
    let old_key = *lost.signing_public_key();
    let mut new_device = Vauchi::in_memory().unwrap();
    new_device.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(new_device);
    engine.navigate_to(AppScreen::Recovery);

    for action in [
        UserAction::ListItemSelected {
            component_id: "recovery_actions".into(),
            item_id: "start_recovery_process".into(),
        },
        UserAction::TextChanged {
            component_id: "old_public_key".into(),
            value: hex::encode(old_key),
        },
        UserAction::ActionPressed {
            action_id: "create_claim".into(),
        },
    ] {
        let _ = engine.handle_action(action);
    }

    let progress = engine
        .vauchi()
        .storage()
        .recovery()
        .load_recovery_progress()
        .unwrap()
        .expect("the recovery is remembered");
    assert_eq!(progress.claim.old_pk(), &old_key);
    assert_eq!(progress.voucher_count(), 0);
}
