// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Before vouching, the helper sees which contact a recovery claim is
//! for, so they can check the person in front of them; a claim for a key
//! that is no contact is refused (vauchi/private#708).

use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::contact_card::ContactCard;
use vauchi_core::{Contact, Identity, SymmetricKey, Vauchi};

fn helper_engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Bob").unwrap();
    AppEngine::new(vauchi)
}

fn add_contact(engine: &AppEngine, identity: &Identity, name: &str) {
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    engine.vauchi().add_contact(contact).unwrap();
}

/// A claim made on the claimant's new device for `lost`.
fn claim_for(lost: &Identity) -> String {
    let mut new_device = Vauchi::in_memory().unwrap();
    new_device.create_identity("New phone").unwrap();
    new_device
        .create_recovery_claim_hex_b64(&hex::encode(lost.signing_public_key()))
        .unwrap()
}

fn paste_claim(engine: &mut AppEngine, claim: String) {
    engine.navigate_to(AppScreen::RecoveryHelp);
    for action in [
        UserAction::ActionPressed {
            action_id: "vouch".into(),
        },
        UserAction::TextChanged {
            component_id: "claim_data".into(),
            value: claim,
        },
        UserAction::ActionPressed {
            action_id: "verify_claim".into(),
        },
    ] {
        let _ = engine.handle_action(action);
    }
}

fn claim_details(engine: &AppEngine) -> Vec<(String, String)> {
    engine
        .current_screen()
        .components
        .into_iter()
        .filter_map(|component| match component {
            Component::InfoPanel { id, items, .. } if id == "claim_details" => Some(items),
            _ => None,
        })
        .flatten()
        .map(|item| (item.title, item.detail))
        .collect()
}

fn claim_input_error(engine: &AppEngine) -> Option<String> {
    engine
        .current_screen()
        .components
        .into_iter()
        .find_map(|component| match component {
            Component::TextInput {
                id,
                validation_error,
                ..
            } if id == "claim_data" => validation_error,
            _ => None,
        })
}

// @internal
#[test]
fn the_confirm_step_names_the_contact_the_claim_is_for() {
    let mut engine = helper_engine();
    let alice = Identity::create("Alice", 0);
    add_contact(&engine, &alice, "Alice");

    paste_claim(&mut engine, claim_for(&alice));

    assert!(claim_details(&engine).contains(&(
        get_string(Locale::ENGLISH, "recovery.claimed_contact_label"),
        "Alice".to_string()
    )));
}

// @internal
#[test]
fn a_claim_for_someone_who_is_no_contact_stays_at_the_paste_step() {
    let mut engine = helper_engine();
    let stranger = Identity::create("Mallory", 0);

    paste_claim(&mut engine, claim_for(&stranger));

    assert!(claim_details(&engine).is_empty());
    assert_eq!(
        claim_input_error(&engine),
        Some(get_string(Locale::ENGLISH, "recovery.claim_not_a_contact"))
    );
}
