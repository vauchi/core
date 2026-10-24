// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! After a shred the person continues on a fresh install
//! (vauchi/private#599). The shred deletes the database the open engine
//! holds; without reopening, a new identity is written into the deleted
//! file and is gone at the next start.

use std::sync::Arc;

use vauchi_platform::{DomainCommand, DomainCommandResult, PlatformAppEngine};

use crate::support::{SharedKeychain, drive_onboarding};

const RELAY: &str = "https://relay.test";

fn open(dir: &tempfile::TempDir, keychain: &SharedKeychain) -> Arc<PlatformAppEngine> {
    PlatformAppEngine::open_with_keychain(
        dir.path().to_string_lossy().to_string(),
        RELAY.into(),
        None,
        Box::new(keychain.clone()),
    )
    .expect("open")
}

// @internal
#[test]
fn after_a_panic_shred_the_next_identity_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = SharedKeychain::new();
    let engine = open(&dir, &keychain);
    drive_onboarding(&engine);
    assert!(matches!(
        engine.dispatch_domain_command(DomainCommand::PanicShred),
        Ok(DomainCommandResult::ShredCompleted { .. })
    ));
    assert!(
        !engine.has_identity().unwrap(),
        "the shred left the identity"
    );

    drive_onboarding(&engine);
    assert!(engine.has_identity().unwrap());
    drop(engine);

    let restarted = open(&dir, &keychain);
    assert!(
        restarted.has_identity().unwrap(),
        "the identity created after the shred did not survive a restart"
    );
}

fn current_surface(engine: &PlatformAppEngine) -> String {
    let batch: serde_json::Value =
        serde_json::from_str(&engine.initial_commands_json().expect("initial commands"))
            .expect("parse command batch");
    batch["commands"]
        .as_array()
        .and_then(|commands| commands.iter().find_map(|c| c.get("SetContextBar")))
        .and_then(|bar| bar["surface_id"].as_str())
        .expect("command batch carries a context bar")
        .to_owned()
}

/// A shell can still report on the screen a shred left: iOS ends the focus
/// of the confirmation field when Welcome replaces it. The reopened engine
/// never showed that screen, and must drop the late event as the shredded
/// engine would have, not fail it (vauchi/private#293).
// @internal
#[test]
fn a_late_event_for_the_screen_before_a_shred_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = SharedKeychain::new();
    let engine = open(&dir, &keychain);
    drive_onboarding(&engine);
    let before_shred = current_surface(&engine);
    assert!(matches!(
        engine.dispatch_domain_command(DomainCommand::PanicShred),
        Ok(DomainCommandResult::ShredCompleted { .. })
    ));
    let fresh = current_surface(&engine);
    assert_ne!(
        fresh, before_shred,
        "precondition: the reopen shows another screen"
    );

    let late = serde_json::json!({
        "InputFocusEnded": { "surface_id": before_shred, "binding_id": "confirm" }
    });
    let reply = engine.dispatch_json(late.to_string());

    let batch: serde_json::Value =
        serde_json::from_str(&reply.expect("a late event is dropped, not an error"))
            .expect("parse command batch");
    assert_eq!(batch["commands"], serde_json::json!([]));
    assert_eq!(
        current_surface(&engine),
        fresh,
        "the late event moved the screen"
    );
}

/// Only screens an engine really showed are inherited: an event for a
/// surface no engine presented still fails closed after the reopen.
// @internal
#[test]
fn an_event_for_a_surface_never_shown_still_fails_closed_after_a_shred() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = SharedKeychain::new();
    let engine = open(&dir, &keychain);
    drive_onboarding(&engine);
    assert!(matches!(
        engine.dispatch_domain_command(DomainCommand::PanicShred),
        Ok(DomainCommandResult::ShredCompleted { .. })
    ));
    let fresh = current_surface(&engine);

    let forged = serde_json::json!({
        "InputFocusEnded": { "surface_id": "never_presented", "binding_id": "confirm" }
    });
    let batch: serde_json::Value = serde_json::from_str(
        &engine
            .dispatch_json(forged.to_string())
            .expect("dispatch returns a batch"),
    )
    .expect("parse command batch");

    assert_eq!(
        batch["commands"][0]["PresentAlert"]["alert"]["title"],
        serde_json::json!("Error"),
        "an unknown surface must still be refused: {batch}"
    );
    assert_eq!(current_surface(&engine), fresh);
}

fn current_revision(engine: &PlatformAppEngine) -> u64 {
    let batch: serde_json::Value =
        serde_json::from_str(&engine.initial_commands_json().expect("initial commands"))
            .expect("parse command batch");
    batch["commands"]
        .as_array()
        .and_then(|commands| commands.iter().find_map(|c| c.get("ReplaceSurface")))
        .and_then(|replace| replace["surface"]["revision"].as_u64())
        .expect("command batch replaces a surface")
}

/// A shell drops an envelope older than what it shows, and every onboarding
/// step is the one `onboarding` surface: a reopened engine counting from 1
/// again left a shell that had just onboarded showing its last step instead
/// of Welcome (iPhone SE, vauchi/private#293). Revisions keep rising across
/// the reopen.
// @internal
#[test]
fn the_first_screen_after_a_shred_is_newer_than_any_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = SharedKeychain::new();
    let engine = open(&dir, &keychain);
    drive_onboarding(&engine);
    let before_shred = current_revision(&engine);
    assert!(matches!(
        engine.dispatch_domain_command(DomainCommand::PanicShred),
        Ok(DomainCommandResult::ShredCompleted { .. })
    ));

    let after_shred = current_revision(&engine);

    assert!(
        after_shred > before_shred,
        "revision went from {before_shred} to {after_shred}; a shell drops it as out of order"
    );
}

/// The engine reopens lazily, on the first call after a shred, and iOS makes
/// two at once: the main thread reloads the screen on `ResetApplication`
/// while its engine queue still delivers the confirmation field's focus
/// event. A caller arriving mid-reopen was told "Storage is locked", and the
/// person saw an error alert over Welcome (iPhone SE, vauchi/private#293).
// @internal
#[test]
fn calls_racing_the_reopen_after_a_shred_wait_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = SharedKeychain::new();
    let engine = open(&dir, &keychain);
    drive_onboarding(&engine);
    assert!(matches!(
        engine.dispatch_domain_command(DomainCommand::PanicShred),
        Ok(DomainCommandResult::ShredCompleted { .. })
    ));

    let callers = 8;
    let start = Arc::new(std::sync::Barrier::new(callers));
    let outcomes: Vec<Result<String, String>> = (0..callers)
        .map(|_| {
            let engine = Arc::clone(&engine);
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                engine
                    .initial_commands_json()
                    .map_err(|error| format!("{error:?}"))
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|caller| caller.join().expect("caller thread"))
        .collect();

    let failures: Vec<&String> = outcomes.iter().filter_map(|o| o.as_ref().err()).collect();
    assert_eq!(
        failures,
        Vec::<&String>::new(),
        "callers racing the reopen failed"
    );
}

fn activate(engine: &PlatformAppEngine, surface: &str) -> serde_json::Value {
    let activated = serde_json::json!({ "SurfaceActivated": { "surface_id": surface } });
    serde_json::from_str(
        &engine
            .dispatch_json(activated.to_string())
            .expect("dispatch returns a batch"),
    )
    .expect("parse command batch")
}

fn is_error_alert(batch: &serde_json::Value) -> bool {
    batch["commands"]
        .as_array()
        .is_some_and(|commands| commands.iter().any(|c| c.get("PresentAlert").is_some()))
}

/// iOS reports its window once, at start, and every screen it brings up as
/// activated. The engine reopened after a shred never heard the window, so
/// it refused each activation and the person saw an error alert on Welcome
/// and on every onboarding step (iPhone SE, vauchi/private#293).
// @internal
#[test]
fn the_window_reported_before_a_shred_still_counts_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = SharedKeychain::new();
    let engine = open(&dir, &keychain);
    let window = serde_json::json!({"PresentationEnvironmentChanged": {
        "available_width": 375, "available_height": 667,
        "input_modes": ["touch"], "motion": "full",
    }});
    engine
        .dispatch_json(window.to_string())
        .expect("environment accepted");
    drive_onboarding(&engine);
    let before_shred = activate(&engine, &current_surface(&engine));
    assert!(
        !is_error_alert(&before_shred),
        "precondition: activation works before the shred: {before_shred}"
    );
    assert!(matches!(
        engine.dispatch_domain_command(DomainCommand::PanicShred),
        Ok(DomainCommandResult::ShredCompleted { .. })
    ));

    let after_shred = activate(&engine, &current_surface(&engine));

    assert!(!is_error_alert(&after_shred), "{after_shred}");
    assert!(
        after_shred["commands"][0]
            .get("SetPresentationProfile")
            .is_some(),
        "{after_shred}"
    );
}

/// After `ResetApplication` the shells drop their state and reload the
/// initial commands; they report their window only when it changes, so the
/// profile has to come with that reload (vauchi/private#678).
// @internal
#[test]
fn the_initial_commands_after_a_shred_carry_the_reported_profile() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = SharedKeychain::new();
    let engine = open(&dir, &keychain);
    let window = serde_json::json!({"PresentationEnvironmentChanged": {
        "available_width": 375, "available_height": 667,
        "input_modes": ["touch"], "motion": "full",
    }});
    engine
        .dispatch_json(window.to_string())
        .expect("environment accepted");
    drive_onboarding(&engine);
    assert!(matches!(
        engine.dispatch_domain_command(DomainCommand::PanicShred),
        Ok(DomainCommandResult::ShredCompleted { .. })
    ));

    let reloaded: serde_json::Value = serde_json::from_str(
        &engine
            .initial_commands_json()
            .expect("initial commands after the shred"),
    )
    .expect("parse initial commands");

    let profiles: Vec<_> = reloaded["commands"]
        .as_array()
        .expect("command list")
        .iter()
        .filter_map(|command| command.get("SetPresentationProfile"))
        .collect();
    assert_eq!(profiles.len(), 1, "{reloaded}");
}
