// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::i18n::Locale;
use vauchi_app::notification_emitter::NotificationEmitter;
use vauchi_app::notification_types::{
    ActivityLogEntry, EventOrigin, NotificationCategory, NotificationPreferences,
};

fn test_name_resolver(contact_id: &str) -> String {
    match contact_id {
        "carol" => "Carol".into(),
        "alice" => "Alice".into(),
        _ => format!("Contact {contact_id}"),
    }
}

fn prefs_all_on() -> NotificationPreferences {
    NotificationPreferences {
        contact_added_enabled: true,
        card_update_enabled: true,
    }
}

fn prefs_all_off() -> NotificationPreferences {
    NotificationPreferences {
        contact_added_enabled: false,
        card_update_enabled: false,
    }
}

// @internal
#[test]
fn duress_always_produces_notification() {
    let entries = vec![(
        "evt-duress".to_string(),
        ActivityLogEntry::DuressAlertReceived {
            contact_id: "carol".to_string(),
        },
    )];

    // Duress, like emergency, is never gated by preferences.
    let results = NotificationEmitter::evaluate(
        &entries,
        &prefs_all_off(),
        Locale::English,
        test_name_resolver,
    );

    assert_eq!(results.len(), 1);
    let n = &results[0];
    assert_eq!(n.event_key, "evt-duress");
    assert_eq!(n.category, NotificationCategory::DuressAlert);
    assert_eq!(n.title, "Duress Alert");
    assert_eq!(n.body, "Carol may be in danger");
    assert_eq!(n.contact_id, "carol");
}

// @internal
#[test]
fn emergency_always_produces_notification() {
    let entries = vec![(
        "evt-001".to_string(),
        ActivityLogEntry::EmergencyAlertReceived {
            contact_id: "carol".to_string(),
        },
    )];

    // Works with prefs all-off — emergency is never gated
    let results = NotificationEmitter::evaluate(
        &entries,
        &prefs_all_off(),
        Locale::English,
        test_name_resolver,
    );

    assert_eq!(results.len(), 1);
    let n = &results[0];
    assert_eq!(n.event_key, "evt-001");
    assert_eq!(n.category, NotificationCategory::EmergencyAlert);
    assert_eq!(n.title, "Emergency Alert");
    assert_eq!(n.body, "Carol sent an emergency alert");
    assert_eq!(n.contact_id, "carol");
}

// @internal
#[test]
fn emergency_uses_name_resolver_for_body() {
    let entries = vec![(
        "evt-002".to_string(),
        ActivityLogEntry::EmergencyAlertReceived {
            contact_id: "alice".to_string(),
        },
    )];

    let results = NotificationEmitter::evaluate(
        &entries,
        &prefs_all_on(),
        Locale::English,
        test_name_resolver,
    );

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].body, "Alice sent an emergency alert");
}

// @internal
#[test]
fn contact_added_synced_with_pref_on_produces_notification() {
    let entries = vec![(
        "evt-003".to_string(),
        ActivityLogEntry::ContactAdded {
            contact_id: "bob".to_string(),
            origin: EventOrigin::Synced,
        },
    )];

    let results = NotificationEmitter::evaluate(
        &entries,
        &prefs_all_on(),
        Locale::English,
        test_name_resolver,
    );

    assert_eq!(results.len(), 1);
    let n = &results[0];
    assert_eq!(n.event_key, "evt-003");
    assert_eq!(n.category, NotificationCategory::ContactAdded);
    assert_eq!(n.title, "Vauchi");
    assert_eq!(n.body, "New contact added");
    assert_eq!(n.contact_id, "bob");
}

// @internal
#[test]
fn contact_added_local_never_notifies() {
    let entries = vec![(
        "evt-004".to_string(),
        ActivityLogEntry::ContactAdded {
            contact_id: "dave".to_string(),
            origin: EventOrigin::Local,
        },
    )];

    // Even with pref on, Local origin must not produce a notification
    let results = NotificationEmitter::evaluate(
        &entries,
        &prefs_all_on(),
        Locale::English,
        test_name_resolver,
    );

    assert!(
        results.is_empty(),
        "Local ContactAdded should never produce a notification"
    );
}

// @internal
#[test]
fn contact_added_pref_off_no_notification() {
    let entries = vec![(
        "evt-005".to_string(),
        ActivityLogEntry::ContactAdded {
            contact_id: "eve".to_string(),
            origin: EventOrigin::Synced,
        },
    )];

    let results = NotificationEmitter::evaluate(
        &entries,
        &prefs_all_off(),
        Locale::English,
        test_name_resolver,
    );

    assert!(
        results.is_empty(),
        "Synced ContactAdded with pref off should not produce a notification"
    );
}

// (Removed `card_update_never_produces_notification`: it asserted the
// placebo behavior — that a contact updating their card produces NO
// notification. M4 S3 makes it the default-on heartbeat; the correct
// behavior is covered by `card_update_produces_notification_when_enabled`,
// `card_update_suppressed_when_disabled`, and `card_update_default_prefs_are_on`.)

// @internal
#[test]
fn multiple_entries_evaluated_independently() {
    let entries = vec![
        (
            "evt-007".to_string(),
            ActivityLogEntry::EmergencyAlertReceived {
                contact_id: "carol".to_string(),
            },
        ),
        (
            "evt-008".to_string(),
            ActivityLogEntry::ContactAdded {
                contact_id: "bob".to_string(),
                origin: EventOrigin::Synced,
            },
        ),
        (
            "evt-009".to_string(),
            ActivityLogEntry::CardUpdateReceived {
                contact_id: "frank".to_string(),
                changed_fields: vec![],
            },
        ),
        (
            "evt-010".to_string(),
            ActivityLogEntry::ContactAdded {
                contact_id: "dave".to_string(),
                origin: EventOrigin::Local,
            },
        ),
    ];

    let results = NotificationEmitter::evaluate(
        &entries,
        &prefs_all_on(),
        Locale::English,
        test_name_resolver,
    );

    // Emergency, synced-contact-added (pref on), and the card-update
    // heartbeat (evt-009, default-on) each fire; the Local contact-add
    // (evt-010) does not. Order is preserved.
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].event_key, "evt-007");
    assert_eq!(results[0].category, NotificationCategory::EmergencyAlert);
    assert_eq!(results[1].event_key, "evt-008");
    assert_eq!(results[1].category, NotificationCategory::ContactAdded);
    assert_eq!(results[2].event_key, "evt-009");
    assert_eq!(results[2].category, NotificationCategory::CardUpdate);
}

// ── M4 S3: CardUpdateReceived heartbeat (default-on, localized) ──────

// @internal
#[test]
fn card_update_produces_notification_when_enabled() {
    let entries = vec![(
        "evt-cu".to_string(),
        ActivityLogEntry::CardUpdateReceived {
            contact_id: "carol".to_string(),
            changed_fields: vec!["phone".to_string()],
        },
    )];

    let results = NotificationEmitter::evaluate(
        &entries,
        &prefs_all_on(),
        Locale::English,
        test_name_resolver,
    );

    assert_eq!(results.len(), 1);
    let n = &results[0];
    assert_eq!(n.event_key, "evt-cu");
    assert_eq!(n.category, NotificationCategory::CardUpdate);
    assert_eq!(n.title, "Vauchi");
    assert_eq!(n.body, "Carol updated their card");
    // contact_id is carried so a tap can deep-link to ContactDetail.
    assert_eq!(n.contact_id, "carol");
}

// @internal
#[test]
fn card_update_suppressed_when_disabled() {
    let entries = vec![(
        "evt-cu".to_string(),
        ActivityLogEntry::CardUpdateReceived {
            contact_id: "carol".to_string(),
            changed_fields: vec![],
        },
    )];
    // prefs_all_off() has card_update_enabled = false.
    let results = NotificationEmitter::evaluate(
        &entries,
        &prefs_all_off(),
        Locale::English,
        test_name_resolver,
    );
    assert!(results.is_empty());
}

// @internal
#[test]
fn card_update_default_prefs_are_on() {
    // The whole point of M4 S3: default preferences enable the card-update
    // heartbeat (the placebo bug was that no such notification existed).
    let entries = vec![(
        "evt-cu".to_string(),
        ActivityLogEntry::CardUpdateReceived {
            contact_id: "carol".to_string(),
            changed_fields: vec![],
        },
    )];
    let results = NotificationEmitter::evaluate(
        &entries,
        &NotificationPreferences::default(),
        Locale::English,
        test_name_resolver,
    );
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].category, NotificationCategory::CardUpdate);
}

// @scenario: notifications :: card-update copy renders in the active locale
// @internal
#[test]
fn card_update_body_localizes_to_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    vauchi_app::i18n::load_locale_from_bytes("de", &bytes).expect("German locale parses");

    let entries = vec![(
        "evt-cu".to_string(),
        ActivityLogEntry::CardUpdateReceived {
            contact_id: "carol".to_string(),
            changed_fields: vec![],
        },
    )];
    let results = NotificationEmitter::evaluate(
        &entries,
        &prefs_all_on(),
        Locale::German,
        test_name_resolver,
    );
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].body, "Carol hat seine Karte aktualisiert");
}

// @internal
#[test]
fn empty_entries_produces_no_notifications() {
    let entries: Vec<(String, ActivityLogEntry)> = vec![];
    let results = NotificationEmitter::evaluate(
        &entries,
        &prefs_all_on(),
        Locale::English,
        test_name_resolver,
    );
    assert!(results.is_empty());
}
