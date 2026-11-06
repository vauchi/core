// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! An expired identity-free claim is shown as expired, is not offered for
//! sharing, and only the supplied renew action issues a new one (#796, P32).

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{AppEngine, AppScreen, WorkflowEngine};
use vauchi_core::clock::Clock;
use vauchi_core::recovery::RecoveryClaim;
use vauchi_core::{Command, Event, Vauchi};

const BEGUN_AT: u64 = 1_800_000_000;

struct TestClock(Mutex<SystemTime>);

impl TestClock {
    fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

impl Clock for TestClock {
    fn now(&self) -> SystemTime {
        *self.0.lock().unwrap()
    }
}

fn begun() -> (Vauchi, Arc<TestClock>, String) {
    let clock = Arc::new(TestClock(Mutex::new(
        UNIX_EPOCH + Duration::from_secs(BEGUN_AT),
    )));
    let phone = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
    let claim = phone.begin_recovery(&"01".repeat(32)).unwrap();
    (phone, clock, claim)
}

fn text(commands: &[Command]) -> String {
    serde_json::to_string(commands).unwrap()
}

fn action_ids(app: &AppEngine) -> Vec<String> {
    app.current_screen()
        .contextual_actions
        .into_iter()
        .map(|action| action.id)
        .collect()
}

fn find_interaction(value: &Value, label: &str) -> Option<Value> {
    match value {
        Value::Object(map) => {
            if map.values().any(|field| field == label)
                && let Some(id) = map.get("interaction_id")
            {
                return Some(id.clone());
            }
            map.values()
                .find_map(|field| find_interaction(field, label))
        }
        Value::Array(items) => items.iter().find_map(|item| find_interaction(item, label)),
        _ => None,
    }
}

fn press(app: &mut AppEngine, commands: &[Command], key: &str) -> Vec<Command> {
    let surface_id = commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.surface_id.clone()),
            _ => None,
        })
        .unwrap();
    let label = get_string(Locale::ENGLISH, key);
    let id = find_interaction(&serde_json::to_value(commands).unwrap(), &label)
        .unwrap_or_else(|| panic!("supplied action missing: {key}"));
    app.dispatch(Event::ActionActivated {
        surface_id,
        interaction_id: serde_json::from_value(id).unwrap(),
    })
    .unwrap()
}

// @internal — P32: showing a live claim leaves it as issued, across engine restarts.
#[test]
fn a_live_claim_is_shown_as_issued_after_time_passes() {
    let (phone, clock, issued) = begun();
    clock.advance(Duration::from_secs(RecoveryClaim::MAX_AGE_SECS));
    let mut app = AppEngine::new(phone);

    let shown = text(&app.initial_commands().unwrap());

    assert_eq!(app.current_app_screen(), &AppScreen::Recovery);
    assert!(shown.contains(&issued));
    assert!(!shown.contains(&get_string(Locale::ENGLISH, "recovery.claim_expired_title")));
    assert_eq!(
        app.vauchi().pending_recovery_claim().unwrap().as_deref(),
        Some(issued.as_str())
    );
}

// @internal — P32: an expired claim cannot be shared; renewing is an explicit, supplied action.
#[test]
fn an_expired_claim_is_withheld_until_the_owner_renews_it() {
    let (phone, clock, issued) = begun();
    let later = RecoveryClaim::MAX_AGE_SECS + 1;
    clock.advance(Duration::from_secs(later));
    let mut app = AppEngine::new(phone);

    let commands = app.initial_commands().unwrap();
    let shown = text(&commands);
    assert_eq!(app.current_app_screen(), &AppScreen::Recovery);
    assert!(shown.contains(&get_string(Locale::ENGLISH, "recovery.claim_expired_title")));
    assert!(!shown.contains(&issued));
    assert_eq!(action_ids(&app), ["renew_claim", "done"]);
    assert_eq!(
        app.vauchi().pending_recovery_claim().unwrap().as_deref(),
        Some(issued.as_str()),
        "showing the expired screen must not renew"
    );

    let after = text(&press(&mut app, &commands, "recovery.create_claim_button"));

    let renewed = app.vauchi().pending_recovery_claim().unwrap().unwrap();
    assert_ne!(renewed, issued);
    let parsed = app.vauchi().parse_recovery_claim_b64(&renewed).unwrap();
    assert_eq!(parsed.timestamp(), BEGUN_AT + later);
    assert!(after.contains(&renewed));
    assert_eq!(action_ids(&app), ["copy_claim", "done"]);
    assert!(!after.contains(&get_string(Locale::ENGLISH, "recovery.claim_expired_title")));
}
