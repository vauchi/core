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

/// Read the retired vocabulary from the ratchet's rules file.
///
/// `humble_check_rules.json` is the single list of retired boundary names —
/// restating it here would let the two enforcement sides drift, and the side
/// that fell behind would report clean. `humble_check.py` fails if a rule
/// greps for a type the list omits, which keeps the list ahead of the rules.
///
/// Resolution mirrors the locales lookup in the contract fixture tests: an
/// explicit CI directory first, then the sibling checkout. Absence is a hard
/// failure — a skipped vocabulary test reports exactly like a clean one.
fn retired_boundary_vocabulary() -> Vec<String> {
    const RULES_RELATIVE: &str = "scripts/scripts/humble_check_rules.json";

    let rules_path = std::env::var_os("VAUCHI_CI_SCRIPTS_DIR")
        .map(|dir| std::path::PathBuf::from(dir).join("scripts/humble_check_rules.json"))
        .filter(|candidate| candidate.is_file())
        .or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .map(|ancestor| ancestor.join(RULES_RELATIVE))
                .find(|candidate| candidate.is_file())
        })
        .expect(
            "domain-vocabulary tests need the scripts checkout — set \
             VAUCHI_CI_SCRIPTS_DIR or clone it beside this repo",
        );

    let rules: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&rules_path).expect("read ratchet rules"))
            .expect("parse ratchet rules");

    let vocabulary: Vec<String> = rules["retired_boundary_vocabulary"]
        .as_array()
        .expect("ratchet rules must carry retired_boundary_vocabulary")
        .iter()
        .map(|term| {
            term.as_str()
                .expect("vocabulary entry is a string")
                .to_owned()
        })
        .collect();

    assert!(
        !vocabulary.is_empty(),
        "retired_boundary_vocabulary is empty — every schema would look clean"
    );
    vocabulary
}

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

    // Fold before matching so a neutralized spelling (`contact_list`,
    // `contactList`) is caught alongside the original.
    let retired: Vec<String> = retired_boundary_vocabulary()
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .collect();

    let offenders: Vec<&String> = identifiers
        .iter()
        .filter(|identifier| {
            let folded = identifier.to_ascii_lowercase().replace(['_', '-'], "");
            retired.iter().any(|term| folded.contains(term))
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
