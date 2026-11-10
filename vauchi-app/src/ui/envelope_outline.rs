// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Envelope outline: a command batch's structure with every text replaced
//! by its length.
//!
//! Two shells that receive the same batch must draw the same structure
//! (ADR-066). When they do not, the question is whether Core sent them
//! something different or one shell mapped it differently; screenshots
//! cannot tell (vauchi/private#1206, UI/UX clarity addendum, step 2). The
//! outline keeps what the diff needs, node kinds, ids, roles, tones and
//! order, and drops what the user typed, so a dev build may log it and a
//! catalog fixture may hold one per screen without carrying a contact's
//! name, number or address.

use serde_json::Value;
use vauchi_core::Command;

/// Keys whose string values are structure, not content: identifiers Core
/// mints and closed vocabularies the contract defines. Everything else that
/// is a string is replaced by `len:<chars>`.
const STRUCTURAL_KEYS: &[&str] = &[
    "id",
    "surface_id",
    "interaction_id",
    "binding_id",
    "code_id",
    "screen_id",
    "icon_token",
    "tone",
    "text_style",
    "role",
    "kind",
    "style",
    "purpose",
    "error_correction",
    "target",
    "shortcut",
    "variant",
    "locale",
    "direction",
];

/// Keys holding media or wire bytes: replaced by `bytes:<count>` whatever
/// their JSON shape, so an avatar never rides along.
const BYTE_KEYS: &[&str] = &["image_data", "data", "bytes", "payload"];

/// The outline of a batch, as the JSON tree of the serialized commands with
/// content redacted. Fails only if a command does not serialize, which the
/// contract's own tests already forbid.
pub fn command_outline(commands: &[Command]) -> Value {
    let value = serde_json::to_value(commands).unwrap_or(Value::Null);
    redact(value, None)
}

/// `command_outline` as one line of JSON, for a log sink.
pub fn outline_json(commands: &[Command]) -> String {
    command_outline(commands).to_string()
}

fn redact(value: Value, key: Option<&str>) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| {
                    let redacted = if BYTE_KEYS.contains(&k.as_str()) {
                        Value::String(format!("bytes:{}", byte_count(&v)))
                    } else {
                        redact(v, Some(k.as_str()))
                    };
                    (k, redacted)
                })
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|item| redact(item, key)).collect())
        }
        Value::String(s) => match key {
            Some(k) if STRUCTURAL_KEYS.contains(&k) => Value::String(s),
            _ => Value::String(format!("len:{}", s.chars().count())),
        },
        other => other,
    }
}

fn byte_count(value: &Value) -> usize {
    match value {
        Value::Array(items) => items.len(),
        Value::String(s) => s.len(),
        Value::Null => 0,
        _ => 1,
    }
}
