// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The screens Core shows before storage opens (vauchi/private#580, ADR-043
//! Amendment 7 §3): a locked keychain asks for the platform prompt and
//! retries; data that cannot be read offers to start over and deletes
//! nothing until the person confirms.

use vauchi_app::i18n::Locale;
use vauchi_app::ui::{StorageLockPresentation, StorageLockReason, StorageLockStep};
use vauchi_core::{ActionSpec, Command, ContextBar, Event, InteractionId, SurfaceId};

fn context_bar(commands: &[Command]) -> (SurfaceId, ContextBar) {
    commands
        .iter()
        .find_map(|command| match command {
            Command::SetContextBar {
                surface_id, bar, ..
            } => Some((surface_id.clone(), (**bar).clone())),
            _ => None,
        })
        .expect("the batch carries a context bar")
}

fn activate(surface_id: &SurfaceId, action: &ActionSpec) -> Event {
    Event::ActionActivated {
        surface_id: surface_id.clone(),
        interaction_id: action.interaction_id.clone(),
    }
}

fn asks_for_prompt(commands: &[Command]) -> bool {
    commands
        .iter()
        .any(|command| matches!(command, Command::RequestBiometricUnlock))
}

fn commands(step: StorageLockStep) -> Vec<Command> {
    match step {
        StorageLockStep::Commands(commands) => commands,
        other => panic!("expected commands, got {other:?}"),
    }
}

fn press_primary(lock: &mut StorageLockPresentation, batch: &[Command]) -> StorageLockStep {
    let (surface, bar) = context_bar(batch);
    lock.dispatch(activate(
        &surface,
        bar.primary.as_ref().expect("primary action"),
    ))
}

fn press_back(lock: &mut StorageLockPresentation, batch: &[Command]) -> StorageLockStep {
    let (surface, bar) = context_bar(batch);
    lock.dispatch(activate(&surface, bar.back.as_ref().expect("back action")))
}

/// Opens the "more actions" menu and activates its only item, Start over.
fn choose_start_over(lock: &mut StorageLockPresentation, batch: &[Command]) -> Vec<Command> {
    let (surface, bar) = context_bar(batch);
    let menu = commands(lock.dispatch(activate(
        &surface,
        bar.secondary.as_ref().expect("a more-actions launcher"),
    )));
    let items = menu
        .iter()
        .find_map(|command| match command {
            Command::PresentOverlay { overlay, .. } => Some(overlay.items.clone()),
            _ => None,
        })
        .expect("the launcher opens the action menu");
    assert_eq!(items.len(), 1, "Start over is the menu's only action");
    commands(lock.dispatch(activate(&surface, &items[0])))
}

// @internal
#[test]
fn a_locked_start_asks_for_the_prompt_and_offers_one_unlock_action() {
    let mut lock = StorageLockPresentation::new(StorageLockReason::Locked, Locale::English);

    let batch = lock.initial_commands();

    assert!(asks_for_prompt(&batch));
    let (_, bar) = context_bar(&batch);
    assert!(bar.primary.as_ref().is_some_and(|action| action.enabled));
    assert_eq!(bar.secondary, None);
}

// @internal
#[test]
fn unlocking_retries_opening_storage() {
    let mut lock = StorageLockPresentation::new(StorageLockReason::Locked, Locale::English);
    let batch = lock.initial_commands();

    assert_eq!(press_primary(&mut lock, &batch), StorageLockStep::RetryOpen);
    assert_eq!(
        lock.dispatch(Event::BiometricUnlockSucceeded),
        StorageLockStep::RetryOpen
    );
}

// @internal
#[test]
fn the_prompt_is_requested_automatically_only_once() {
    let mut lock = StorageLockPresentation::new(StorageLockReason::Locked, Locale::English);

    let first = lock.initial_commands();
    let second = lock.initial_commands();

    assert!(asks_for_prompt(&first));
    assert!(!asks_for_prompt(&second));
}

// @internal
#[test]
fn a_tap_whose_retry_finds_storage_still_locked_asks_for_the_prompt() {
    let mut lock = StorageLockPresentation::new(StorageLockReason::Locked, Locale::English);
    let batch = lock.initial_commands();
    assert_eq!(press_primary(&mut lock, &batch), StorageLockStep::RetryOpen);

    let still_locked = lock.show(StorageLockReason::Locked);

    assert!(asks_for_prompt(&still_locked));
}

// @internal
#[test]
fn a_prompt_whose_retry_finds_storage_still_locked_does_not_prompt_again() {
    let mut lock = StorageLockPresentation::new(StorageLockReason::Locked, Locale::English);
    lock.initial_commands();
    assert_eq!(
        lock.dispatch(Event::BiometricUnlockSucceeded),
        StorageLockStep::RetryOpen
    );

    let still_locked = lock.show(StorageLockReason::Locked);

    assert!(
        !asks_for_prompt(&still_locked),
        "a prompt that cannot unlock storage must not loop"
    );
}

// @internal
#[test]
fn unreadable_data_offers_try_again_first() {
    let mut lock = StorageLockPresentation::new(StorageLockReason::Unreadable, Locale::English);
    let unreadable = lock.initial_commands();

    assert_eq!(
        press_primary(&mut lock, &unreadable),
        StorageLockStep::RetryOpen
    );
}

// @internal
#[test]
fn unreadable_data_deletes_nothing_until_the_person_confirms() {
    let mut lock = StorageLockPresentation::new(StorageLockReason::Unreadable, Locale::English);
    let unreadable = lock.initial_commands();
    assert!(!asks_for_prompt(&unreadable));

    let confirm = choose_start_over(&mut lock, &unreadable);

    assert_eq!(
        press_primary(&mut lock, &confirm),
        StorageLockStep::StartOverConfirmed
    );
}

// @internal
#[test]
fn cancelling_the_confirmation_goes_back_without_starting_over() {
    let mut lock = StorageLockPresentation::new(StorageLockReason::Unreadable, Locale::English);
    let unreadable = lock.initial_commands();
    let confirm = choose_start_over(&mut lock, &unreadable);

    let back = commands(press_back(&mut lock, &confirm));

    assert_eq!(context_bar(&back).0, context_bar(&unreadable).0);
    assert_eq!(press_primary(&mut lock, &back), StorageLockStep::RetryOpen);
}

// @internal
#[test]
fn unreadable_data_is_not_reopened_by_a_prompt_or_a_stale_action() {
    let mut lock = StorageLockPresentation::new(StorageLockReason::Unreadable, Locale::English);
    let unreadable = lock.initial_commands();
    let (surface, _) = context_bar(&unreadable);
    let forged = [
        Event::BiometricUnlockSucceeded,
        Event::ActionActivated {
            surface_id: surface.clone(),
            interaction_id: InteractionId::new("storage_lock.confirm_start_over").unwrap(),
        },
        Event::ActionActivated {
            surface_id: SurfaceId::new("somewhere.else").unwrap(),
            interaction_id: InteractionId::new("anything").unwrap(),
        },
    ];

    for event in forged {
        let step = lock.dispatch(event.clone());

        assert!(
            matches!(step, StorageLockStep::Commands(_)),
            "{event:?} must not retry or start over, got {step:?}"
        );
    }
}

// @internal
#[test]
fn a_locked_start_that_turns_out_unreadable_stops_asking_for_the_prompt() {
    let mut lock = StorageLockPresentation::new(StorageLockReason::Locked, Locale::English);
    lock.initial_commands();

    let batch = lock.show(StorageLockReason::Unreadable);

    assert!(!asks_for_prompt(&batch));
    assert!(matches!(
        lock.dispatch(Event::BiometricUnlockSucceeded),
        StorageLockStep::Commands(_)
    ));
}

// @internal
#[test]
fn every_storage_lock_screen_has_its_strings() {
    let mut lock = StorageLockPresentation::new(StorageLockReason::Locked, Locale::English);
    let locked = lock.initial_commands();
    let unreadable = lock.show(StorageLockReason::Unreadable);
    let confirm = choose_start_over(&mut lock, &unreadable);

    for (name, batch) in [
        ("locked", locked),
        ("unreadable", unreadable),
        ("confirm", confirm),
    ] {
        let rendered = format!("{batch:?}");
        assert!(
            !rendered.contains("Missing:"),
            "{name} screen renders a missing string: {rendered}"
        );
    }
}
