// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! On Hover both phones read each other's codes at the same time, and
//! that is the proof that they are close. The audio check is a second
//! signal there: when it fails, the exchange goes on
//! (vauchi/private#1300, owner decision 2026-11-19).
//!
//! Two AppEngines on one fake clock, each reading the other's frames; no
//! wall-clock waits (CC-06).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_app::ui::{AppEngine, Component, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::clock::FakeClock;

use crate::hover_engines::{engine_on_hover, own_qr_data, scan_into};

const TICK: Duration = Duration::from_millis(500);
const AUDIO_FAILURE_TITLE: &str = "Couldn't confirm devices are close";

fn status_titles(engine: &AppEngine) -> Vec<String> {
    engine
        .current_screen()
        .components
        .iter()
        .filter_map(|component| match component {
            Component::StatusIndicator { title, .. } => Some(title.clone()),
            _ => None,
        })
        .collect()
}

fn hears_nothing(engine: &mut AppEngine) {
    engine
        .dispatch(Event::AudioSamplesRecorded {
            samples: vec![0.0; 44_100],
            sample_rate: 44_100,
        })
        .expect("a recording is a valid event during Hover");
}

fn saved_contacts(engine: &AppEngine) -> usize {
    engine.vauchi().list_contacts().unwrap().len()
}

// @internal
#[test]
fn a_hover_exchange_completes_after_both_phones_heard_nothing() {
    let clock = Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    let mut alice = engine_on_hover("Alice", clock.clone());
    let mut bob = engine_on_hover("Bob", clock.clone());
    let mut failure_shown = false;
    let mut both_saved = false;

    for tick in 0..600 {
        alice.poll_notifications();
        bob.poll_notifications();
        let (a_frame, b_frame) = (own_qr_data(&alice), own_qr_data(&bob));
        if let Some(frame) = b_frame {
            scan_into(&mut alice, frame);
        }
        if let Some(frame) = a_frame {
            scan_into(&mut bob, frame);
        }
        if tick == 4 {
            hears_nothing(&mut alice);
            hears_nothing(&mut bob);
        }
        failure_shown |= status_titles(&alice)
            .iter()
            .any(|t| t == AUDIO_FAILURE_TITLE)
            || status_titles(&bob).iter().any(|t| t == AUDIO_FAILURE_TITLE);
        both_saved = saved_contacts(&alice) == 1 && saved_contacts(&bob) == 1;
        if both_saved {
            break;
        }
        clock.advance(TICK);
    }

    assert!(!failure_shown, "a Hover phone showed the audio failure");
    assert!(both_saved, "the exchange did not complete");
}
