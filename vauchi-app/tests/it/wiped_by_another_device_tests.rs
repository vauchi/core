// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A device another of the identity's devices told to wipe returns to
//! setup after the sync that brought the instruction (vauchi/private#916).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::Vauchi;
use vauchi_core::api::VauchiConfig;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::sync::SyncItem;

// @scenario: privacy_compliance :: When I leave with the wipe choice my other devices wipe
#[test]
fn a_device_told_to_wipe_resets_after_its_sync() {
    // Relay addresses that lead nowhere: the sync must not leave this
    // machine (the default relay is production).
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url("http://127.0.0.1:9")
        .with_ohttp_relay_url("http://127.0.0.1:9");
    let mut vauchi = Vauchi::new(config).unwrap();
    vauchi.create_identity("Ada").unwrap();
    vauchi
        .apply_sync_items(vec![SyncItem::WipeThisDevice {
            timestamp: 1_700_000_000,
        }])
        .unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Settings);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "sync_now".into(),
    });

    assert_eq!(
        (
            matches!(result, ActionResult::WipeComplete),
            engine.vauchi().identity().is_none()
        ),
        (true, true)
    );
}
