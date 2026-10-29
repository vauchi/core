// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A device another of the identity's devices told to wipe returns to
//! setup after the sync that brought the instruction (vauchi/private#916).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::Vauchi;
use vauchi_core::sync::SyncItem;

// @internal
#[test]
fn a_device_told_to_wipe_resets_after_its_sync() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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
