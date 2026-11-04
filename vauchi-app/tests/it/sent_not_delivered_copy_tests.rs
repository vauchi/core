// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! ADR-057 amendment 2026-11-04: the sender sees at most queued, sent or
//! stored on the relay, never that a contact's device received an update.
//! The copy shown after a send, and for an empty delivery queue, says
//! sent (private#1065).

use vauchi_app::i18n::Locale;
use vauchi_app::notification_types::ActivityLogEntry;
use vauchi_app::ui::{
    ActivityLogEngine, ActivityLogItem, ContactDetailEngine, DeliverySummary, Field, Item,
    WorkflowEngine,
};
use vauchi_app::{aha_moment_message_localized, aha_moment_title_localized};
use vauchi_core::AhaMomentType;

fn screen_text(json: &str) -> String {
    json.to_lowercase()
}

// @internal
#[test]
fn a_sent_card_update_reads_sent_in_the_activity_log() {
    let engine = ActivityLogEngine::new(vec![ActivityLogItem {
        event_key: "card_delivered:c1:m1".into(),
        entry: ActivityLogEntry::CardUpdateDelivered {
            contact_id: "c1".into(),
        },
        contact_name: "Ada".into(),
        created_at: 1_700_000_000,
    }]);

    let json = serde_json::to_string(&engine.current_screen()).unwrap();

    assert!(json.contains("Card update sent to Ada"), "{json}");
    assert!(!screen_text(&json).contains("deliver"), "{json}");
}

// @internal
#[test]
fn the_first_send_tip_says_sent() {
    let moment = AhaMomentType::FirstOutboundDelivered;

    assert_eq!(
        aha_moment_title_localized(moment, Locale::ENGLISH),
        "Update sent"
    );
    assert_eq!(
        aha_moment_message_localized(moment, Locale::ENGLISH),
        "Your contacts get it the next time their device checks in."
    );
    assert_eq!(moment.title(), "Update sent");
    assert!(!moment.message().to_lowercase().contains("now have"));
}

// @internal
#[test]
fn a_contact_with_nothing_pending_shows_nothing_waiting() {
    let contact = Item {
        id: "c1".into(),
        name: "Ada".into(),
        subtitle: None,
        initials: "A".into(),
        status: None,
        actions: vec![],
        a11y: None,
    };
    let engine = ContactDetailEngine::new(contact, Vec::<Field>::new(), String::new())
        .with_delivery_summary(DeliverySummary {
            total: 2,
            delivered: 0,
            pending: 0,
            failed: 0,
        });

    let json = serde_json::to_string(&engine.current_screen()).unwrap();

    assert!(json.contains("Nothing waiting to send"), "{json}");
    assert!(!screen_text(&json).contains("all delivered"), "{json}");
}
