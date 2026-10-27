// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Help answer on end-to-end encryption, opened the way the shells
//! report a tap.

use crate::event_path_support::{activate, row};
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::Command;
use vauchi_core::api::Vauchi;

// @scenario: privacy_compliance :: Understand E2E encryption
#[test]
fn help_explains_that_only_my_contacts_and_not_the_relay_can_read_my_data() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Help);
    let help = engine.initial_commands().unwrap();
    let (surface_id, question) = row(&help, "What is end-to-end encryption?");

    let shown = activate(&mut engine, surface_id, &question);

    let answer = shown
        .iter()
        .find_map(|command| match command {
            Command::PresentOverlay { overlay, .. } => overlay.body.clone(),
            _ => None,
        })
        .expect("the answer opens");
    for promise in [
        "only you and your contact can read",
        "The relay server sees only encrypted blobs",
        "cannot read names, fields, or any content",
    ] {
        assert!(
            answer.contains(promise),
            "{promise:?} missing from {answer:?}"
        );
    }
}
