// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A repeat Hover exchange where one phone still holds the contact and the
//! other does not (vauchi/private#1285): the phone that holds it asks its
//! person, the other waits, and a contact's key changes only after a yes.
//!
//! Two AppEngines on fake clocks, each reading the other's frames; no
//! wall-clock waits (CC-06).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_app::ui::{AppEngine, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::clock::FakeClock;
use vauchi_core::platform::{Command, ContextBar, Event, InputMode, MotionPreference, SurfaceId};
use vauchi_core_testkit::meeting;

use crate::hover_engines::{engine_on_hover_with_profile, own_qr_data, scan_into};

const ACTIONS: &str = "exchange_actions";
const ACCEPT: &str = "reconnect_accept";
const DECLINE: &str = "reconnect_decline";
const WAITING: &str = "reconnect_waiting";
const KEPT_BY_YOU: &str = "reconnect_kept_by_you";
const KEPT_BY_THEM: &str = "reconnect_kept_by_them";
const TIMED_OUT: &str = "reconnect_timed_out";
const TICK: Duration = Duration::from_millis(500);

struct World {
    alice: AppEngine,
    bob: AppEngine,
    clock_a: Arc<FakeClock>,
    clock_b: Arc<FakeClock>,
    alice_id: String,
    bob_id: String,
    old_key: Option<[u8; 32]>,
}

enum Lost {
    Nobody,
    BobLostAlice,
}

fn profile(name: &str, clock: &Arc<FakeClock>) -> Vauchi {
    let mut vauchi = Vauchi::in_memory_with_clock(clock.clone()).expect("in-memory Vauchi");
    vauchi.create_identity(name).expect("identity");
    vauchi
}

fn id_of(vauchi: &Vauchi) -> String {
    hex::encode(vauchi.identity().unwrap().signing_public_key())
}

fn key_of(engine: &AppEngine, contact_id: &str) -> Option<[u8; 32]> {
    engine
        .vauchi()
        .get_contact(contact_id)
        .unwrap()
        .and_then(|contact| contact.shared_key().map(|key| *key.as_bytes()))
}

/// Alice and Bob on the Hover screen. `met` says whether they exchanged
/// before, and `lost` which of them no longer holds the other.
fn world(met: bool, lost: Lost) -> World {
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let clock_a = Arc::new(FakeClock::new(start));
    let clock_b = Arc::new(FakeClock::new(start));
    let a = profile("Alice", &clock_a);
    let b = profile("Bob", &clock_b);
    let (alice_id, bob_id) = (id_of(&a), id_of(&b));
    if met {
        meeting::meet(&a, &b, true);
    }
    if matches!(lost, Lost::BobLostAlice) {
        assert!(b.remove_contact(&alice_id).unwrap());
    }
    let alice = engine_on_hover_with_profile(a);
    let bob = engine_on_hover_with_profile(b);
    let old_key = key_of(&alice, &bob_id);
    World {
        alice,
        bob,
        clock_a,
        clock_b,
        alice_id,
        bob_id,
        old_key,
    }
}

fn shows(engine: &AppEngine, id: &str) -> bool {
    serde_json::to_string(&engine.current_screen())
        .unwrap()
        .contains(&format!("\"{id}\""))
}

fn press(engine: &mut AppEngine, item: &str) {
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: ACTIONS.into(),
        item_id: item.into(),
    });
}

/// Each tick both phones show a frame, each reads the other's, and both
/// clocks move on. Stops when `done` holds; `false` if it never did.
fn drive(w: &mut World, ticks: usize, mut done: impl FnMut(&World) -> bool) -> bool {
    for _ in 0..ticks {
        w.alice.poll_notifications();
        w.bob.poll_notifications();
        let (a_frame, b_frame) = (own_qr_data(&w.alice), own_qr_data(&w.bob));
        if let Some(frame) = b_frame {
            scan_into(&mut w.alice, frame);
        }
        if let Some(frame) = a_frame {
            scan_into(&mut w.bob, frame);
        }
        if done(w) {
            return true;
        }
        w.clock_a.advance(TICK);
        w.clock_b.advance(TICK);
    }
    false
}

fn alice_is_asked(w: &World) -> bool {
    shows(&w.alice, ACCEPT)
}

/// Bob lost Alice, they hover again, and Alice's phone has put the
/// question.
fn alice_asked_about_bob() -> World {
    let mut w = world(true, Lost::BobLostAlice);
    assert!(
        drive(&mut w, 600, alice_is_asked),
        "Alice's phone never asked"
    );
    w
}

// @scenario: contact_exchange :: Meeting again asks before starting a new connection
// @internal
#[test]
fn the_phone_that_kept_the_contact_asks_and_the_other_waits_with_nothing_saved() {
    let mut w = alice_asked_about_bob();

    let still_asking_ten_seconds_later = !drive(&mut w, 20, |w| !alice_is_asked(w));

    assert!(still_asking_ten_seconds_later);
    assert!(shows(&w.alice, DECLINE));
    assert!(shows(&w.bob, WAITING));
    assert!(!shows(&w.bob, ACCEPT));
    assert_eq!(key_of(&w.alice, &w.bob_id), w.old_key);
    assert_eq!(w.bob.vauchi().list_contacts().unwrap().len(), 0);
    let shown_while_asking = own_qr_data(&w.alice).expect("the code stays on screen");
    assert_eq!(&shown_while_asking[..4], "CNF5");
}

// A screen that scrolls takes the sync chip, and a chip that changes moves
// what is under it: here the code the other phone is reading
// (`2026-06-03-exchange-qr-scan-stability`, vauchi/private#1304).
// @internal
#[test]
fn the_question_screen_has_no_sync_chip_over_the_code() {
    let w = alice_asked_about_bob();

    assert!(shows(&w.alice, "reconnect_question"));
    assert!(!shows(&w.alice, "sync"));
}

// @scenario: contact_exchange :: Starting a new connection after meeting again
// @internal
#[test]
fn a_yes_gives_both_phones_the_new_exchanges_key() {
    let mut w = alice_asked_about_bob();

    press(&mut w.alice, ACCEPT);
    let bob_saved = drive(&mut w, 200, |w| key_of(&w.bob, &w.alice_id).is_some());

    assert!(bob_saved, "Bob never saved Alice after her yes");
    let (alice_key, bob_key) = (key_of(&w.alice, &w.bob_id), key_of(&w.bob, &w.alice_id));
    assert_eq!(alice_key, bob_key);
    assert_ne!(alice_key, w.old_key);
    assert!(!shows(&w.alice, ACCEPT));
    assert!(!shows(&w.bob, WAITING));
}

// @scenario: contact_exchange :: Leaving the old connection after meeting again
// @internal
#[test]
fn a_no_leaves_the_key_as_it_was_and_the_other_phone_saves_nothing() {
    let mut w = alice_asked_about_bob();

    press(&mut w.alice, DECLINE);
    let bob_was_told = drive(&mut w, 200, |w| shows(&w.bob, KEPT_BY_THEM));

    assert!(bob_was_told, "Bob's phone never said Alice declined");
    assert!(shows(&w.alice, KEPT_BY_YOU));
    assert!(!shows(&w.alice, ACCEPT));
    assert_eq!(key_of(&w.alice, &w.bob_id), w.old_key);
    assert_eq!(w.bob.vauchi().list_contacts().unwrap().len(), 0);
}

// @scenario: contact_exchange :: Nobody answers the reconnect question
// @internal
#[test]
fn no_answer_ends_as_a_no_at_two_minutes_and_not_before() {
    let mut w = alice_asked_about_bob();

    let ended_early = drive(&mut w, 200, |w| !alice_is_asked(w));
    let still_waiting_at_100_seconds = shows(&w.bob, WAITING);
    let both_ended = drive(&mut w, 120, |w| {
        shows(&w.alice, TIMED_OUT) && (shows(&w.bob, TIMED_OUT) || shows(&w.bob, KEPT_BY_THEM))
    });

    assert!(!ended_early, "the question ended before its deadline");
    assert!(still_waiting_at_100_seconds);
    assert!(both_ended, "the question outlived its deadline");
    assert_eq!(key_of(&w.alice, &w.bob_id), w.old_key);
    assert_eq!(w.bob.vauchi().list_contacts().unwrap().len(), 0);
}

// @internal
#[test]
fn a_second_tap_changes_nothing() {
    let mut w = alice_asked_about_bob();

    press(&mut w.alice, DECLINE);
    press(&mut w.alice, ACCEPT);
    let bob_was_told = drive(&mut w, 200, |w| shows(&w.bob, KEPT_BY_THEM));

    assert!(bob_was_told);
    assert_eq!(key_of(&w.alice, &w.bob_id), w.old_key);
    assert_eq!(w.bob.vauchi().list_contacts().unwrap().len(), 0);
}

// @scenario: contact_exchange :: Two phones that still have each other are not asked
// @internal
#[test]
fn two_phones_that_still_hold_each_other_are_not_asked_and_keep_their_key() {
    let mut w = world(true, Lost::Nobody);
    let mut asked = false;

    let both_done = drive(&mut w, 600, |w| {
        asked |= shows(&w.alice, ACCEPT) || shows(&w.bob, ACCEPT);
        asked |= shows(&w.alice, WAITING) || shows(&w.bob, WAITING);
        shows(&w.alice, "done") && shows(&w.bob, "done")
    });

    assert!(both_done);
    assert!(!asked);
    assert_eq!(key_of(&w.alice, &w.bob_id), w.old_key);
    assert_eq!(key_of(&w.bob, &w.alice_id), w.old_key);
}

// @internal
#[test]
fn two_phones_that_never_met_are_not_asked_and_both_save() {
    let mut w = world(false, Lost::Nobody);
    let mut asked = false;

    let both_saved = drive(&mut w, 600, |w| {
        asked |= shows(&w.alice, ACCEPT) || shows(&w.bob, ACCEPT);
        asked |= shows(&w.alice, WAITING) || shows(&w.bob, WAITING);
        key_of(&w.alice, &w.bob_id).is_some() && key_of(&w.bob, &w.alice_id).is_some()
    });

    assert!(both_saved);
    assert!(!asked);
    assert_eq!(key_of(&w.alice, &w.bob_id), key_of(&w.bob, &w.alice_id));
}

// @internal
#[test]
fn a_blocked_contact_is_not_asked_about_and_nothing_is_saved_or_rekeyed() {
    let mut w = world(true, Lost::BobLostAlice);
    w.alice.vauchi().block_contact(&w.bob_id).unwrap();
    let mut asked = false;

    drive(&mut w, 300, |w| {
        asked |= shows(&w.alice, ACCEPT);
        false
    });

    assert!(!asked);
    assert_eq!(key_of(&w.alice, &w.bob_id), w.old_key);
    assert_eq!(w.bob.vauchi().list_contacts().unwrap().len(), 0);
}

/// The shell's environment report; `pins_actions` is whether it keeps
/// pinned actions in view.
fn report_environment(engine: &mut AppEngine, pins_actions: bool) {
    engine
        .dispatch(Event::PresentationEnvironmentChanged {
            available_width: 375,
            available_height: 760,
            input_modes: vec![InputMode::Touch],
            motion: MotionPreference::Full,
            color_scheme: None,
            pinned_actions: pins_actions,
        })
        .expect("environment accepted");
}

/// What the shell is handed for the screen the engine is on: the surface
/// as its wire text, and the bar.
fn presented_again(engine: &mut AppEngine) -> (String, SurfaceId, ContextBar) {
    let commands = engine
        .dispatch(Event::PresentationInvalidated)
        .expect("surface presented again");
    let surface = commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(serde_json::to_string(surface).unwrap()),
            _ => None,
        })
        .expect("a surface");
    let (surface_id, bar) = commands
        .into_iter()
        .find_map(|command| match command {
            Command::SetContextBar {
                surface_id, bar, ..
            } => Some((surface_id, *bar)),
            _ => None,
        })
        .expect("a context bar");
    (surface, surface_id, bar)
}

fn presented(engine: &mut AppEngine, pins_actions: bool) -> (String, SurfaceId, ContextBar) {
    report_environment(engine, pins_actions);
    presented_again(engine)
}

fn labels(bar: &ContextBar) -> Vec<&str> {
    bar.pinned
        .iter()
        .map(|action| action.label.as_str())
        .collect()
}

const YES: &str = "Start a new connection";
const NO: &str = "Leave it";

/// Whether the surface's wire text has an element labelled exactly so. The
/// question's own sentence begins with the words of the yes.
fn has_label(surface: &str, label: &str) -> bool {
    surface.contains(&format!("\"{label}\""))
}

// On an iPhone SE both answers were below the fold and the person at the
// phone did not find the second one (rig, 2026-11-20, vauchi/private#1304).
// @internal
#[test]
fn a_shell_that_pins_actions_gets_both_answers_in_the_bar_and_not_in_the_content() {
    let mut w = alice_asked_about_bob();

    let (surface, _, bar) = presented(&mut w.alice, true);

    assert_eq!(labels(&bar), [YES, NO]);
    assert!(!has_label(&surface, YES), "the yes is still in the content");
    assert!(!has_label(&surface, NO), "the no is still in the content");
    assert!(surface.contains("reconnect_question"));
}

// @internal
#[test]
fn a_shell_that_does_not_pin_actions_keeps_both_answers_in_the_content() {
    let mut w = alice_asked_about_bob();

    let (surface, _, bar) = presented(&mut w.alice, false);

    assert_eq!(labels(&bar), [] as [&str; 0]);
    assert!(has_label(&surface, YES));
    assert!(has_label(&surface, NO));
}

// @internal
#[test]
fn the_pinned_no_leaves_the_key_as_it_was_and_takes_the_answers_away() {
    let mut w = alice_asked_about_bob();
    let (_, surface_id, bar) = presented(&mut w.alice, true);
    let no = bar.pinned[1].interaction_id.clone();

    w.alice
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id: no,
        })
        .expect("the pinned answer is accepted");
    let bob_was_told = drive(&mut w, 200, |w| shows(&w.bob, KEPT_BY_THEM));

    assert!(bob_was_told, "Bob's phone never said Alice declined");
    assert!(shows(&w.alice, KEPT_BY_YOU));
    assert_eq!(key_of(&w.alice, &w.bob_id), w.old_key);
    let (_, _, bar) = presented(&mut w.alice, true);
    assert_eq!(labels(&bar), [] as [&str; 0]);
}

// An engine that replaces another (after an unlock or a wipe) is not told
// the environment again; the shell would lose its pinned answers.
// @internal
#[test]
fn an_engine_that_takes_over_keeps_what_the_shell_said_about_pinning() {
    let mut w = alice_asked_about_bob();
    report_environment(&mut w.bob, true);
    let window = w.bob.reported_window();

    w.alice.inherit_reported_window(window);

    let (_, _, bar) = presented_again(&mut w.alice);
    assert_eq!(labels(&bar), [YES, NO]);
}
