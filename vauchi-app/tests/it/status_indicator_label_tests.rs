// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Component::StatusIndicator` must carry a core-resolved localized
//! badge label (`status_label`) so frontends never derive text from the
//! `Status` discriminant. Before this field, every frontend invented
//! the badge text (hardcoded English on Android/iOS — W-class leaks in
//! `2026-07-06-mobile-domain-shell-violations`; slice of
//! `2026-07-07-code-quality-debt-sweep` §2). The discriminant itself
//! stays on the wire for the sanctioned theme-palette color mapping,
//! mirroring `IndicatorKind`.

use vauchi_app::i18n::{Locale, get_string, load_locale_from_bytes};
use vauchi_app::ui::{Component, LinkResponderEngine, Status, WorkflowEngine};
use vauchi_core::exchange::link_mode::{initiator_generate, parse_exchange_deep_link};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

const ALL_STATUSES: [Status; 5] = [
    Status::Pending,
    Status::InProgress,
    Status::Success,
    Status::Failed,
    Status::Warning,
];

// @scenario: component_serialization :: status badge label resolves in every locale
// @internal
#[test]
fn status_label_key_resolves_for_every_variant() {
    load_german();
    for status in ALL_STATUSES {
        for locale in [Locale::English, Locale::German] {
            let label = get_string(locale, status.label_key());
            assert!(
                !label.is_empty() && !label.starts_with("Missing:"),
                "no catalog entry for {status:?} in {locale:?}: {label}"
            );
        }
    }
}

// @scenario: component_serialization :: status badge label resolves exact German
// @internal
#[test]
fn status_label_key_resolves_exact_german() {
    load_german();
    assert_eq!(
        get_string(Locale::German, Status::Success.label_key()),
        "Erfolg"
    );
    assert_eq!(
        get_string(Locale::German, Status::Pending.label_key()),
        "Ausstehend"
    );
    assert_eq!(
        get_string(Locale::German, Status::InProgress.label_key()),
        "In Bearbeitung"
    );
}

// @scenario: component_serialization :: StatusIndicator carries status_label on the wire
// @internal
#[test]
fn status_indicator_wire_json_carries_status_label() {
    let component = Component::StatusIndicator {
        id: "si".to_string(),
        icon: None,
        title: "Linking".to_string(),
        detail: None,
        status: Status::Success,
        status_label: "Erfolg".to_string(),
        a11y: None,
    };

    let json = serde_json::to_value(&component).expect("serialize StatusIndicator");
    assert_eq!(json["StatusIndicator"]["status_label"], "Erfolg");

    let roundtrip: Component = serde_json::from_value(json).expect("deserialize StatusIndicator");
    assert_eq!(component, roundtrip);
}

// @scenario: component_serialization :: pre-status_label payloads still deserialize
// @internal
#[test]
fn status_indicator_deserializes_legacy_payload_without_label() {
    let legacy = r#"{"StatusIndicator":{"id":"si","icon":null,"title":"T","detail":null,"status":"Pending","a11y":null}}"#;
    let parsed: Component = serde_json::from_str(legacy).expect("legacy payload parses");
    match parsed {
        Component::StatusIndicator { status_label, .. } => assert_eq!(status_label, ""),
        other => panic!("expected StatusIndicator, got {other:?}"),
    }
}

// Relocated from the Android renderer (CC-24): the catalog-follows-locale
// behavior lives in core now that core resolves the badge label.
// @scenario: link-responder :: waiting badge label is catalog-resolved
// @internal
#[test]
fn link_responder_waiting_badge_is_catalog_resolved_german() {
    load_german();
    let (init, _) = initiator_generate();
    let payload = parse_exchange_deep_link(&init.url).expect("canonical URL parses");
    let engine = LinkResponderEngine::new(payload).with_locale(Locale::German);

    let screen = engine.current_screen();
    let badge_label = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::StatusIndicator {
                status_label,
                status,
                ..
            } => {
                assert_eq!(*status, Status::InProgress);
                Some(status_label.clone())
            }
            _ => None,
        })
        .expect("waiting screen renders a StatusIndicator");
    assert_eq!(badge_label, "In Bearbeitung");
}
