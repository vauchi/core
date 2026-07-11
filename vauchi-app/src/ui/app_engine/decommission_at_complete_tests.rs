// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine-level tests for the device-replacement decommission outcome
//! (`2026-03-23-device-replacement-flow`).
//!
//! Confirming "remove old device" on the source wizard must actually
//! decommission this device — wipe its contact ratchet sessions so it
//! can no longer advance a chain the replacement now owns — while
//! "keep both" must leave the sessions untouched.

use super::{AppEngine, AppScreen};
use crate::ui::device_replacement::DeviceReplacementEngine;
use crate::ui::{ActionResult, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{DoubleRatchetState, SymmetricKey, X3DHKeyPair};

/// An `AppEngine` whose source-side replacement wizard has finished the
/// transfer and sits on the Complete step, with one live ratchet session.
fn app_with_replacement_at_complete() -> (AppEngine, String) {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let app_contact = Contact::from_exchange(
        [0x41u8; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    let contact_id = app_contact.id().to_string();
    vauchi.add_contact(app_contact).expect("contact");
    let their_dh = X3DHKeyPair::generate();
    let ratchet =
        DoubleRatchetState::initialize_initiator(&SymmetricKey::generate(), *their_dh.public_key())
            .expect("ratchet");
    vauchi
        .save_ratchet_state(&contact_id, &ratchet)
        .expect("session saved");

    let mut app = AppEngine::new(vauchi);
    let mut wizard = DeviceReplacementEngine::new_source();
    wizard.peer_connected("123456".into());
    let _ = wizard.handle_action(UserAction::ActionPressed {
        action_id: "confirm".into(),
    });
    wizard.sync_complete(1, 1);
    app.engine = Box::new(wizard);
    app.screen = AppScreen::DeviceReplacement;
    (app, contact_id)
}

fn press(app: &mut AppEngine, action_id: &str) -> ActionResult {
    app.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

// @internal
#[test]
fn confirm_remove_old_device_wipes_ratchet_sessions() {
    let (mut app, contact_id) = app_with_replacement_at_complete();

    let _ = press(&mut app, "decommission");
    let _ = press(&mut app, "remove_old");
    let _ = press(&mut app, "confirm_remove");

    assert!(
        app.vauchi()
            .get_ratchet_state(&contact_id)
            .expect("ratchet query")
            .is_none(),
        "confirmed decommission must wipe this device's ratchet sessions"
    );
}

// @internal
#[test]
fn keep_both_devices_leaves_ratchet_sessions_intact() {
    let (mut app, contact_id) = app_with_replacement_at_complete();

    let _ = press(&mut app, "decommission");
    let _ = press(&mut app, "keep_both");

    assert!(
        app.vauchi()
            .get_ratchet_state(&contact_id)
            .expect("ratchet query")
            .is_some(),
        "keep-both must not touch this device's ratchet sessions"
    );
}
