// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Scheduling and cancelling the deletion from the privacy screen reaches
//! my other devices (vauchi/private#723).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::Vauchi;
use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::identity::DeviceInfo;
use vauchi_core::sync::SyncItem;

fn privacy_with_a_tablet() -> (AppEngine, [u8; 32]) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    let identity = vauchi.identity().unwrap();
    let seed = *identity.master_seed();
    let tablet = DeviceInfo::derive(&seed, 1, "tablet".into(), 0);
    let mut registry = identity.initial_device_registry();
    registry
        .add_device(tablet.to_registered(&seed), identity.signing_keypair())
        .unwrap();
    vauchi
        .storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Privacy);
    (engine, *tablet.device_id())
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn journaled(engine: &AppEngine, tablet: &[u8; 32]) -> Vec<&'static str> {
    let vauchi = engine.vauchi();
    let registry = vauchi
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap();
    DeviceSyncOrchestrator::load(
        vauchi.storage(),
        vauchi.identity().unwrap().create_device_info(0),
        registry,
    )
    .unwrap()
    .pending_for_device(tablet)
    .iter()
    .filter_map(|item| match item {
        SyncItem::DeletionScheduled { .. } => Some("scheduled"),
        SyncItem::DeletionCancelled { .. } => Some("cancelled"),
        _ => None,
    })
    .collect()
}

// @internal
#[test]
fn scheduling_then_cancelling_from_the_privacy_screen_reaches_my_tablet() {
    let (mut engine, tablet) = privacy_with_a_tablet();

    let _ = press(&mut engine, "delete");
    let _ = press(&mut engine, "confirm_delete");
    // With a tablet linked, leaving first asks whether it wipes too (#916).
    let _ = press(&mut engine, "keep_other_devices");
    let after_schedule = journaled(&engine, &tablet);
    engine.navigate_to(AppScreen::Privacy);
    let _ = press(&mut engine, "cancel_deletion");

    assert_eq!(
        (after_schedule, journaled(&engine, &tablet)),
        (vec!["scheduled"], vec!["scheduled", "cancelled"])
    );
}

// @internal
#[test]
fn leaving_with_wipe_from_the_privacy_screen_stores_the_choice() {
    let (mut engine, _tablet) = privacy_with_a_tablet();

    let _ = press(&mut engine, "delete");
    let _ = press(&mut engine, "confirm_delete");
    let _ = press(&mut engine, "wipe_other_devices");

    assert!(
        vauchi_core::api::DeletionManager::new(engine.vauchi().storage())
            .scheduled_wipes_other_devices()
            .unwrap()
    );
}
