// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! One notice direction against schedules that a second implementation,
//! written from the protocol text alone, replays too (vauchi/private#1281).
//! The two must decide the same: epoch numbers, alarms, what the sender
//! keeps, how many records each writes, which updates arrive.

use serde_json::{Value, json};
use vauchi_core::crypto::X25519KeyPair;
use vauchi_core::notice::direction::{Receiver, Sender};
use vauchi_core::notice::keys::{Label, seed_epoch};
use vauchi_core::notice::seal::NOTICE_BYTES;

type Notice = [u8; NOTICE_BYTES];

fn json(path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("json")
}

fn key(inputs: &Value, name: &str) -> X25519KeyPair {
    let bytes: [u8; 32] = hex::decode(inputs[name].as_str().expect("hex"))
        .expect("hex")
        .try_into()
        .expect("32 bytes");
    X25519KeyPair::from_bytes(bytes)
}

fn names(alarms: &[vauchi_core::notice::direction::Alarm]) -> Vec<String> {
    alarms.iter().map(|alarm| format!("{alarm:?}")).collect()
}

fn run(schedule: &Value, inputs: &Value, start: u64) -> Value {
    let sender_device = key(inputs, "s_device_private");
    let receiver_device = key(inputs, "r_device_private");
    let label = Label::new(sender_device.public_key(), receiver_device.public_key());
    let handshake = key(inputs, "s_handshake_private")
        .diffie_hellman(key(inputs, "r_handshake_private").public_key())
        .expect("contributory");
    let sender_public = *sender_device.public_key();
    let receiver_public = *receiver_device.public_key();
    let mut sender = Sender::new(
        seed_epoch(&handshake, &label),
        label.clone(),
        sender_device,
        receiver_public,
        start,
    );
    let mut receiver = Receiver::new(
        seed_epoch(&handshake, &label),
        label,
        receiver_device,
        sender_public,
        start,
    );

    let mut written: Vec<Vec<Notice>> = Vec::new();
    let mut observed = Vec::new();
    for step in schedule["steps"].as_array().expect("steps") {
        let hour = step["hour"].as_i64().expect("hour");
        let now = start
            .checked_add_signed(hour * 3_600)
            .expect("a time after 1970");
        let flips: Vec<u64> = step["flip_from"]
            .as_array()
            .map(|f| f.iter().filter_map(Value::as_u64).collect())
            .unwrap_or_default();
        let mut delivered = Vec::new();
        for from in step["deliver_from"].as_array().expect("deliver_from") {
            let from = from.as_u64().expect("index");
            for notice in &written[usize::try_from(from).expect("index")] {
                let mut notice = *notice;
                if flips.contains(&from) {
                    notice[40] ^= 1;
                }
                delivered.push(notice);
            }
        }
        let acknowledge = step["acknowledge"].as_bool().expect("acknowledge");
        let mut updates_read = Vec::new();
        let this = if step["device"] == "S" {
            if let Some(edit) = step["edit"].as_u64() {
                let byte = u8::try_from(edit).expect("edit fits a byte");
                sender.queue_update(&[byte; 32], &[byte; 31]);
            }
            sender.read(&delivered, now);
            let out = sender.write(now);
            if acknowledge {
                sender.acknowledge();
            }
            out
        } else {
            for read in receiver.read(&delivered, now) {
                updates_read.push(read.update_key[0]);
            }
            let out = receiver.write(now);
            if acknowledge {
                receiver.acknowledge();
            }
            out
        };
        observed.push(json!({
            "written": this.len(),
            "updates_read": updates_read,
            "r_current": receiver.current_number(),
            "r_newest": receiver.newest_number(),
            "r_alarms": names(receiver.alarms()),
            "s_writes_in": sender.writes_in_number(),
            "s_kept": sender.kept_numbers(),
            "s_alarms": names(sender.alarms()),
        }));
        written.push(this);
    }
    json!({ "name": schedule["name"], "observed": observed })
}

// @internal
#[test]
fn the_direction_decides_as_the_independent_implementation_does() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/test-vectors/");
    let doc = json(&format!("{fixtures}notice-schedules-v1.json"));
    let inputs = json(&format!("{fixtures}notice-core-vectors-v8.json"))["inputs"].clone();
    let start = doc["start_unix_seconds"].as_u64().expect("start");
    let actual: Vec<Value> = doc["schedules"]
        .as_array()
        .expect("schedules")
        .iter()
        .map(|schedule| run(schedule, &inputs, start))
        .collect();

    let out =
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("notice-conformance-rust.json");
    std::fs::write(&out, serde_json::to_string_pretty(&actual).expect("json")).expect("written");

    let expected: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(format!("{fixtures}notice-conformance-expected-v1.json"))
            .expect("the other implementation's decisions are checked in"),
    )
    .expect("json");
    assert_eq!(expected.len(), actual.len(), "schedules");
    for (want, got) in expected.iter().zip(&actual) {
        assert_eq!(want["name"], got["name"]);
        let want = want["observed"].as_array().expect("observed");
        let got = got["observed"].as_array().expect("observed");
        assert_eq!(want.len(), got.len(), "steps of {}", got.len());
        for (index, (w, g)) in want.iter().zip(got).enumerate() {
            assert_eq!(w, g, "step {index}");
        }
    }
}
