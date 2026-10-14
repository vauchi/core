// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The event notifier delivers on its own thread and merges events that
//! pile up (vauchi/private#677).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use vauchi_app::event_notifier::EventNotifier;

// @internal
#[test]
fn events_that_pile_up_arrive_merged_and_none_is_lost() {
    let deliveries: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
    let sink = Arc::clone(&deliveries);
    let notifier = EventNotifier::spawn(move |screens| sink.lock().unwrap().push(screens)).unwrap();

    for i in 0..100 {
        notifier.notify([format!("screen-{}", i % 5)]);
    }
    notifier.flush();

    let deliveries = deliveries.lock().unwrap();
    assert!(
        (1..=100).contains(&deliveries.len()),
        "{} deliveries",
        deliveries.len()
    );
    let seen: BTreeSet<&String> = deliveries.iter().flatten().collect();
    assert_eq!(seen.len(), 5, "every affected screen reaches the listener");
}

// @internal
#[test]
fn delivery_runs_on_another_thread() {
    let caller = std::thread::current().id();
    let delivered_on = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&delivered_on);
    let notifier = EventNotifier::spawn(move |_| {
        *sink.lock().unwrap() = Some(std::thread::current().id());
    })
    .unwrap();

    notifier.notify(std::iter::empty::<String>());
    notifier.flush();

    let delivered_on = delivered_on.lock().unwrap().expect("delivered");
    assert_ne!(delivered_on, caller);
}
