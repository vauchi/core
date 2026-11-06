// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Generates `recovery_ceremony_v1.json` from the ceremony scenarios
//! (vauchi/private#796, plan step 5). Real crypto makes every run's bytes
//! differ, so a transcript keeps framing, lengths and all presentation, and
//! replaces random bytes and strings with fixed fill of the same type. Packet
//! counts vary with compression, so a transcript only marks where it omits
//! some. A
//! shell can still decode and execute every command, and regeneration is
//! byte-identical, so comparing with a fresh run replays the ceremony.

#[path = "../../../vauchi-app/tests/fixture_json/mod.rs"]
#[allow(dead_code)]
mod fixture_json;

use std::cell::RefCell;
use std::path::PathBuf;

use serde_json::{Value, json};
use vauchi_core::{Command, Event};

/// Whole transfers run to hundreds of packets; the first few show the
/// framing, and the rest differ only in their offsets.
const RECORDED_PACKETS: usize = 3;
/// Magic, piece index and offset. The piece length after them varies by a few
/// bytes, because a backup of random keys compresses differently on each run.
const DATA_HEADER_KEPT_BYTES: usize = 8;
const ACK_BYTES: usize = 8;
const SHORTEST_KEY_PREFIX: usize = 8;

#[derive(Default)]
struct Recording {
    active: bool,
    paused: bool,
    elided: usize,
    steps: Vec<Value>,
    secrets: Vec<(String, String)>,
}

thread_local! {
    static RECORDING: RefCell<Recording> = RefCell::default();
}

pub(super) fn record(actor: &str, event: Option<&Event>, commands: &[Command]) {
    RECORDING.with_borrow_mut(|recording| {
        if recording.active && !recording.paused {
            recording.steps.push(json!({
                "actor": actor,
                "event": event,
                "commands": commands,
            }));
        }
    });
}

pub(super) fn register_secret(secret: &str, placeholder: &str) {
    RECORDING.with_borrow_mut(|recording| {
        recording
            .secrets
            .push((secret.to_owned(), placeholder.to_owned()));
    });
}

/// A screen may show any prefix of a key, so each prefix is a secret too,
/// longest first so a short one cannot split a longer match.
pub(super) fn register_key(key_hex: &str, placeholder: &str) {
    for length in (SHORTEST_KEY_PREFIX..=key_hex.len()).rev() {
        register_secret(&key_hex[..length], placeholder);
    }
}

pub(super) fn note_packet(packet: usize) {
    RECORDING.with_borrow_mut(|recording| {
        if packet == RECORDED_PACKETS {
            recording.paused = true;
        }
        if packet >= RECORDED_PACKETS {
            recording.elided += 1;
        }
    });
}

/// Resumes recording when a transfer ends and notes how many packets the
/// transcript left out.
pub(super) struct Elision;

impl Elision {
    pub(super) fn begin() -> Self {
        Self
    }
}

impl Drop for Elision {
    fn drop(&mut self) {
        RECORDING.with_borrow_mut(|recording| {
            recording.paused = false;
            if recording.active && recording.elided > 0 {
                recording.steps.push(json!({"elided_packets": true}));
            }
            recording.elided = 0;
        });
    }
}

fn fill_bytes(bytes: &mut [Value], keep: usize) {
    for byte in bytes.iter_mut().skip(keep) {
        *byte = json!(0);
    }
}

fn starts_with_magic(bytes: &[Value], magic: &[u8; 4]) -> bool {
    bytes.len() >= magic.len()
        && bytes
            .iter()
            .zip(magic)
            .all(|(byte, expected)| byte.as_u64() == Some(u64::from(*expected)))
}

fn sanitize(value: &mut Value, secrets: &[(String, String)]) {
    match value {
        Value::String(text) => {
            for (secret, placeholder) in secrets {
                if text.contains(secret.as_str()) {
                    *text = text.replace(secret.as_str(), placeholder);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| sanitize(item, secrets)),
        Value::Object(fields) => {
            for (name, field) in fields.iter_mut() {
                if let (true, Value::Array(bytes)) = (
                    matches!(name.as_str(), "data" | "payload" | "adv_data"),
                    &mut *field,
                ) {
                    let keep = if starts_with_magic(bytes, b"RBP1") {
                        DATA_HEADER_KEPT_BYTES
                    } else if starts_with_magic(bytes, b"RBA1") {
                        ACK_BYTES
                    } else {
                        0
                    };
                    fill_bytes(bytes, keep);
                } else {
                    sanitize(field, secrets);
                }
            }
        }
        _ => {}
    }
}

fn transcript_of(scenario: fn()) -> Vec<Value> {
    RECORDING.with_borrow_mut(|recording| {
        *recording = Recording {
            active: true,
            ..Recording::default()
        };
    });
    scenario();
    let recording = RECORDING.take();
    let mut steps = Value::Array(recording.steps);
    let mut secrets = recording.secrets;
    secrets.sort_by_key(|(secret, _)| std::cmp::Reverse(secret.len()));
    sanitize(&mut steps, &secrets);
    match steps {
        Value::Array(steps) => steps,
        _ => Vec::new(),
    }
}

fn fresh_fixture() -> Value {
    let scenarios: [(&str, fn()); 5] = [
        (
            "Social recovery restores the same identity on a new device",
            super::social_recovery_restores_the_same_identity_through_the_ceremony,
        ),
        (
            "A recovery backup crosses a connection that accepts small parts",
            super::a_backup_crosses_a_link_at_the_smallest_mtu,
        ),
        (
            "A busy recovery connection reports an incomplete transfer",
            super::a_busy_link_reports_an_incomplete_transfer_and_keeps_the_share,
        ),
        (
            "An interrupted backup transfer resumes after a restart",
            super::an_interrupted_transfer_resumes_after_a_restart,
        ),
        (
            "Cancelling a backup transfer ignores late parts",
            super::a_cancelled_transfer_stops_and_ignores_late_parts,
        ),
    ];
    let scenarios: Vec<Value> = scenarios
        .into_iter()
        .map(|(name, scenario)| json!({"scenario": name, "steps": transcript_of(scenario)}))
        .collect();
    json!({"schema_version": 1, "scenarios": scenarios})
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../vauchi-app/fixtures/journeys/recovery_ceremony_v1.json")
}

fn differing_scenario(recorded: &Value, fresh: &Value) -> String {
    let names = |fixture: &Value, index: usize| fixture["scenarios"][index].clone();
    (0..5)
        .find(|index| names(recorded, *index) != names(fresh, *index))
        .map_or("the envelope".to_owned(), |index| {
            fresh["scenarios"][index]["scenario"].to_string()
        })
}

// @internal — a transcript that differs between runs could not be diffed in CI.
#[test]
fn the_ceremony_transcript_is_the_same_on_every_run() {
    let first = fresh_fixture();
    let second = fresh_fixture();

    assert!(
        first == second,
        "random data leaked into {}",
        differing_scenario(&first, &second)
    );
}

// @internal — the checked-in corpus is what the ceremony does now.
#[test]
fn the_checked_in_ceremony_fixture_matches_a_fresh_run() {
    let recorded: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture_path()).expect("read fixture"))
            .expect("fixture is JSON");
    let fresh = fresh_fixture();

    assert!(
        recorded == fresh,
        "ceremony fixture drifted in {}; review and run the ignored regeneration",
        differing_scenario(&recorded, &fresh)
    );
}

// @internal — every recorded command and event still decodes as its contract type.
#[test]
fn the_checked_in_ceremony_fixture_decodes_as_the_contract() {
    let recorded: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture_path()).expect("read fixture"))
            .expect("fixture is JSON");

    let mut steps = 0;
    for scenario in recorded["scenarios"].as_array().unwrap() {
        for step in scenario["steps"].as_array().unwrap() {
            if step.get("elided_packets").is_some() {
                continue;
            }
            let _: Option<Event> = serde_json::from_value(step["event"].clone()).unwrap();
            let _: Vec<Command> = serde_json::from_value(step["commands"].clone()).unwrap();
            steps += 1;
        }
    }
    assert!(steps > 20, "only {steps} steps recorded");
}

// @internal
#[test]
#[ignore = "regenerates Core's recovery ceremony fixture"]
fn regenerate_ceremony_fixture() {
    let json = serde_json::to_string_pretty(&fresh_fixture()).expect("serialize fixture");
    let collapsed = fixture_json::collapse_number_arrays(&json);
    std::fs::write(fixture_path(), format!("{collapsed}\n")).expect("write ceremony fixture");
}
