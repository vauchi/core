// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{Event, InputValue};

/// Maximum JSON payload accepted by the generic presentation dispatch path.
///
/// 64 KiB holds every presentation event because the byte-bearing variants
/// (`ImageReceived`, `FilePickedFromUser`, `AudioSamplesRecorded`, and the
/// BLE/NFC payloads) reach Core through the typed `handle_hardware_event`
/// seam rather than this JSON path. That seam is the `split_dispatch_api`
/// debt ADR-066 drives to zero — when it is retired those variants move
/// here, and this ceiling must be re-derived from the largest legitimate
/// payload (a user-picked backup, `MAX_FULL_BACKUP_BYTES`) before it is.
pub const MAX_EVENT_JSON_BYTES: usize = 64 * 1024;
/// Maximum object/array nesting accepted before event deserialization.
pub const MAX_EVENT_JSON_NESTING_DEPTH: usize = 16;
/// Maximum raw text or choice value accepted from a presented input.
pub const MAX_EVENT_INPUT_VALUE_BYTES: usize = 4 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EventJsonError {
    #[error("event JSON exceeds {MAX_EVENT_JSON_BYTES} bytes")]
    TooLarge,
    #[error("event JSON exceeds nesting depth {MAX_EVENT_JSON_NESTING_DEPTH}")]
    TooDeep,
    #[error("event input value exceeds {MAX_EVENT_INPUT_VALUE_BYTES} bytes")]
    InputValueTooLarge,
    #[error("event JSON is malformed")]
    Malformed,
}

/// Parse one bounded generic event received from a presentation shell.
pub fn event_from_json(json: &str) -> Result<Event, EventJsonError> {
    if json.len() > MAX_EVENT_JSON_BYTES {
        return Err(EventJsonError::TooLarge);
    }
    if exceeds_nesting_depth(json) {
        return Err(EventJsonError::TooDeep);
    }

    let event = serde_json::from_str(json).map_err(|_| EventJsonError::Malformed)?;
    validate_event(&event)?;
    Ok(event)
}

fn validate_event(event: &Event) -> Result<(), EventJsonError> {
    if let Event::ValueChanged { value, .. } = event {
        let value_length = match value {
            InputValue::Text(value) => value.len(),
            InputValue::Choice(Some(value)) => value.len(),
            InputValue::Choice(None) | InputValue::Boolean(_) | InputValue::Number(_) => 0,
        };
        if value_length > MAX_EVENT_INPUT_VALUE_BYTES {
            return Err(EventJsonError::InputValueTooLarge);
        }
    }
    Ok(())
}

fn exceeds_nesting_depth(json: &str) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for byte in json.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_EVENT_JSON_NESTING_DEPTH {
                    return true;
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }

    false
}
