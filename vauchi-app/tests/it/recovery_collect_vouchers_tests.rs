// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A user who lost every device shares a claim, takes each helper's
//! voucher in, and sends the proof once enough have come in. Before
//! private#877 the app had no way to take a voucher in, and "Submit
//! proof" showed success without sending anything.

use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
#[cfg(feature = "testing")]
use vauchi_core::VauchiConfig;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::{Contact, Identity, SymmetricKey, Vauchi};

#[cfg(feature = "testing")]
use crate::recording_relay::recording_relay;

/// The recovery threshold a fresh identity starts with.
const THRESHOLD: usize = 3;

struct Claimant {
    engine: AppEngine,
    lost: Identity,
    claim: String,
}

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

/// Walks the new device from the recovery intro to the shared claim.
fn claimant_on(mut vauchi: Vauchi) -> Claimant {
    vauchi.create_identity("Alice").unwrap();
    let lost = Identity::create("Alice", 0);
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Recovery);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "recovery_actions".into(),
        item_id: "start_recovery_process".into(),
    });
    type_into(
        &mut engine,
        "old_public_key",
        &hex::encode(lost.signing_public_key()),
    );
    let _ = press(&mut engine, "create_claim");
    let claim = engine
        .current_screen()
        .components
        .into_iter()
        .find_map(|component| match component {
            Component::Text { id, content, .. } if id == "claim_data" => Some(content),
            _ => None,
        })
        .expect("the claim is shown");
    Claimant {
        engine,
        lost,
        claim,
    }
}

fn claimant() -> Claimant {
    claimant_on(Vauchi::in_memory().unwrap())
}

/// A claimant whose relay is `relay_url`.
#[cfg(feature = "testing")]
fn claimant_with_relay(relay_url: &str) -> (Claimant, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut config = VauchiConfig::default().with_relay_url(relay_url);
    config.ohttp.allow_direct = true;
    config.relay.connect_timeout_ms = 2_000;
    config.storage_path = dir.path().join("new-device.db");
    (claimant_on(Vauchi::new(config).unwrap()), dir)
}

impl Claimant {
    /// A voucher from a helper who knows the lost identity as a contact.
    fn voucher_from(&self, helper_name: &str) -> String {
        let mut helper = Vauchi::in_memory().unwrap();
        helper.create_identity(helper_name).unwrap();
        helper
            .add_contact(Contact::from_exchange(
                *self.lost.signing_public_key(),
                ContactCard::new("Alice"),
                SymmetricKey::generate(),
                0,
            ))
            .unwrap();
        helper.create_voucher_from_claim_b64(&self.claim).unwrap()
    }

    fn add_voucher(&mut self, voucher: &str) -> ActionResult {
        type_into(&mut self.engine, "voucher_input", voucher);
        press(&mut self.engine, "add_voucher")
    }

    fn collect_from(&mut self, helpers: &[&str]) {
        let _ = press(&mut self.engine, "collect_vouchers");
        for helper in helpers {
            let voucher = self.voucher_from(helper);
            let _ = self.add_voucher(&voucher);
        }
    }

    fn stored_voucher_count(&self) -> Option<usize> {
        self.engine
            .vauchi()
            .get_recovery_progress()
            .unwrap()
            .map(|progress| progress.voucher_count())
    }

    fn voucher_input(&self) -> Option<(String, Option<String>)> {
        self.engine
            .current_screen()
            .components
            .into_iter()
            .find_map(|component| match component {
                Component::TextInput {
                    id,
                    value,
                    validation_error,
                    ..
                } if id == "voucher_input" => Some((value, validation_error)),
                _ => None,
            })
    }
}

fn en(key: &str) -> String {
    get_string(Locale::ENGLISH, key)
}

// @internal
#[test]
fn the_shared_claim_leads_on_to_collecting_vouchers() {
    let mut alice = claimant();

    let _ = press(&mut alice.engine, "collect_vouchers");

    let screen = alice.engine.current_screen();
    assert_eq!(screen.title, en("recovery.collecting_vouchers_title"));
    assert_eq!(alice.voucher_input(), Some((String::new(), None)));
}

// @internal
#[test]
fn each_helpers_voucher_is_added_and_counted() {
    let mut alice = claimant();

    alice.collect_from(&["Bob", "Carol"]);

    assert_eq!(alice.stored_voucher_count(), Some(2));
    let screen = alice.engine.current_screen();
    let progress = screen.progress.expect("progress toward the threshold");
    assert_eq!(
        (progress.current_step, progress.total_steps),
        (2, THRESHOLD as u8)
    );
    let collected = screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::ActionList { id, items } if id == "collected_vouchers" => Some(items.len()),
            _ => None,
        });
    assert_eq!(collected, Some(2));
    assert_eq!(
        alice.voucher_input(),
        Some((String::new(), None)),
        "the input clears for the next voucher"
    );
}

// @internal
#[test]
fn a_voucher_that_does_not_fit_is_refused_inline() {
    let mut alice = claimant();
    let someone_else = claimant();
    let _ = press(&mut alice.engine, "collect_vouchers");

    for not_alices in [
        "not a voucher".to_string(),
        alice.claim.clone(),
        someone_else.voucher_from("Dave"),
    ] {
        let _ = alice.add_voucher(&not_alices);

        assert_eq!(
            alice.voucher_input().and_then(|(_, error)| error),
            Some(en("recovery.voucher_not_accepted")),
            "{not_alices:.20}"
        );
    }
    assert_eq!(alice.stored_voucher_count(), Some(0));
}

// The loopback relay is reached directly, which core allows only in its
// `testing` build (`build_relay_transport`); production goes over OHTTP.
// @scenario: contact_recovery :: Upload recovery proof to relay
#[cfg(feature = "testing")]
#[test]
fn enough_vouchers_send_the_proof_to_the_relay() {
    let (relay_url, received) = recording_relay();
    let (mut alice, _dir) = claimant_with_relay(&relay_url);
    alice.collect_from(&["Bob", "Carol", "Dave"]);

    let _ = press(&mut alice.engine, "submit_proof");

    let (path, body) = received
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the proof reached the relay");
    assert!(path.contains("recovery"), "posted to {path}");
    assert!(!body.is_empty());
    assert_eq!(
        alice.engine.current_screen().title,
        en("recovery.complete_title")
    );
    assert_eq!(
        alice.stored_voucher_count(),
        None,
        "the proof is on the relay; nothing is left in progress"
    );
}

// Gated like the success test: only there does the upload reach for the
// closed port, so the failure has the cause this test names.
// @internal
#[cfg(feature = "testing")]
#[test]
fn a_proof_the_relay_cannot_take_keeps_the_vouchers() {
    // A closed port, so the upload fails by construction.
    let (mut alice, _dir) = claimant_with_relay("http://127.0.0.1:1");
    alice.collect_from(&["Bob", "Carol", "Dave"]);

    let result = press(&mut alice.engine, "submit_proof");

    let ActionResult::ShowAlert { message, .. } = result else {
        panic!("expected an alert, got {result:?}");
    };
    assert_eq!(message, en("recovery.proof_upload_failed"));
    assert_eq!(
        alice.engine.current_screen().title,
        en("recovery.collecting_vouchers_title")
    );
    assert_eq!(alice.stored_voucher_count(), Some(THRESHOLD));
}

// @internal
#[test]
fn a_restarted_app_resumes_collecting_with_the_vouchers_so_far() {
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(dir.path().join("new-device.db"))
        .with_storage_key(SymmetricKey::generate());
    let mut alice = claimant_on(Vauchi::new(config.clone()).unwrap());
    alice.collect_from(&["Bob", "Carol"]);
    drop(alice.engine);

    let mut vauchi = Vauchi::new(config).unwrap();
    vauchi.refresh_identity_from_storage();
    let mut restarted = AppEngine::new(vauchi);
    restarted.navigate_to(AppScreen::Recovery);

    let screen = restarted.current_screen();
    assert_eq!(screen.title, en("recovery.collecting_vouchers_title"));
    let progress = screen.progress.expect("progress toward the threshold");
    assert_eq!(
        (progress.current_step, progress.total_steps),
        (2, THRESHOLD as u8)
    );
    let collected = screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::ActionList { id, items } if id == "collected_vouchers" => Some(items.len()),
            _ => None,
        });
    assert_eq!(collected, Some(2));
}
