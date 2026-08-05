// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Domain-vocabulary boundary tests (ADR-066).
//!
//! `Command` and `Event` are the only two envelopes that cross the shell
//! boundary, and neither may name a Vauchi feature or domain concept. The
//! ratchet in `humble_check.py` greps *frontend* sources for retired names;
//! it cannot see the Core-owned protocol those frontends decode. These tests
//! close that gap from the other side.
//!
//! Asserting over the generated schema rather than a fixture corpus is
//! deliberate: the schema enumerates every variant and field, so a newly
//! added `ContactList` command fails here even when no fixture exercises it.
//!
//! Run with:
//!   cargo test --features schema-gen -p vauchi-core --test domain_vocabulary_boundary_tests

#![cfg(feature = "schema-gen")]

use schemars::schema_for;
use vauchi_core::{Command, Event};

/// Retired public-boundary vocabulary, from the ADR-066 enforcement list in
/// `wire-humble.md`. These are matched case-insensitively against every
/// variant tag and property name so that neutralized spellings (`contact_list`,
/// `contactList`) are caught alongside the original.
const RETIRED_BOUNDARY_VOCABULARY: &[&str] = &[
    "screenmodel",
    "workflowengine",
    "useraction",
    "actionresult",
    "uifieldvisibility",
    "visibilitymode",
    "fieldvisibilitychanged",
    "contactlist",
    "recoveryscreen",
];

/// Collect every object key and every string in an `enum`/`const` position —
/// the two places serde puts a variant tag — from a JSON Schema document.
fn schema_identifiers(value: &serde_json::Value, found: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                found.push(key.clone());
                if key == "enum" || key == "const" {
                    collect_strings(child, found);
                }
                schema_identifiers(child, found);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                schema_identifiers(item, found);
            }
        }
        _ => {}
    }
}

fn collect_strings(value: &serde_json::Value, found: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => found.push(text.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_strings(item, found);
            }
        }
        _ => {}
    }
}

/// `known_variant` is a positive control: a tag the walker must find. Without
/// it a walker that stopped reaching variants would report a clean schema, and
/// the test would pass by inspecting nothing.
fn assert_free_of_domain_vocabulary(
    envelope: &str,
    schema: serde_json::Value,
    known_variant: &str,
) {
    let mut identifiers = Vec::new();
    schema_identifiers(&schema, &mut identifiers);

    assert!(
        identifiers.iter().any(|found| found == known_variant),
        "{envelope} walk did not reach the known variant `{known_variant}`, \
         so a clean result would prove nothing"
    );

    let offenders: Vec<&String> = identifiers
        .iter()
        .filter(|identifier| {
            let folded = identifier.to_ascii_lowercase().replace(['_', '-'], "");
            RETIRED_BOUNDARY_VOCABULARY
                .iter()
                .any(|retired| folded.contains(retired))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "{envelope} exposes retired domain vocabulary at the shell boundary \
         (ADR-066): {offenders:?}"
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Shell boundary carries no domain vocabulary
// @scenario: generic_presentation_protocol.feature :: Shell boundary carries no domain vocabulary
#[test]
fn test_command_schema_carries_no_domain_vocabulary() {
    let schema = serde_json::to_value(schema_for!(Command)).expect("serialize command schema");
    assert_free_of_domain_vocabulary("Command", schema, "QrDisplay");
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Shell boundary carries no domain vocabulary
// @scenario: generic_presentation_protocol.feature :: Shell boundary carries no domain vocabulary
#[test]
fn test_event_schema_carries_no_domain_vocabulary() {
    let schema = serde_json::to_value(schema_for!(Event)).expect("serialize event schema");
    assert_free_of_domain_vocabulary("Event", schema, "QrScanned");
}
