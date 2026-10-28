// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A scheduled deletion can only be cancelled during its week of grace;
//! Delete now appears once the week is over.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::{DeletionManager, Vauchi};
use vauchi_core::clock::Clock;

const DAY: u64 = 24 * 60 * 60;

/// A wall clock the test moves by hand, so a week passes without waiting.
struct HandClock(AtomicU64);

impl Clock for HandClock {
    fn now(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}

impl HandClock {
    fn advance_days(&self, days: u64) {
        self.0.fetch_add(days * DAY, Ordering::SeqCst);
    }
}

fn privacy_actions(engine: &mut AppEngine) -> Vec<String> {
    engine
        .navigate_to(AppScreen::Privacy)
        .contextual_actions
        .iter()
        .map(|a| a.id.clone())
        .collect()
}

// @scenario: emergency_shred :: Soft shred cannot proceed before grace period expires
#[test]
fn delete_now_appears_only_once_the_week_of_grace_is_over() {
    let clock = Arc::new(HandClock(AtomicU64::new(1_700_000_000)));
    let mut vauchi = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
    vauchi.create_identity("Alice").unwrap();
    DeletionManager::new(vauchi.storage())
        .schedule_deletion()
        .unwrap();
    let mut engine = AppEngine::new(vauchi);

    clock.advance_days(6);
    let during_grace = privacy_actions(&mut engine);
    clock.advance_days(1);
    let after_grace = privacy_actions(&mut engine);

    assert!(
        during_grace.contains(&"cancel_deletion".to_string()),
        "{during_grace:?}"
    );
    assert!(
        !during_grace.contains(&"execute_deletion".to_string()),
        "{during_grace:?}"
    );
    assert!(
        after_grace.contains(&"execute_deletion".to_string()),
        "{after_grace:?}"
    );
}
