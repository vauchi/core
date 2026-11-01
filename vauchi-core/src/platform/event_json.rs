// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{Event, InputValue};

/// Maximum JSON payload accepted by the generic presentation dispatch path.
///
/// Every shell event, byte-bearing hardware events included, travels this
/// path (ADR-066 retires the typed hardware-event seam), so the
/// ceiling is derived from the largest legitimate payload: a user-picked
/// file of [`MAX_FILE_PICK_BYTES`]. Serde spells each byte as up to
/// four JSON characters (`255,`); the envelope allowance covers the variant
/// tag, field names, and the filename.
pub const MAX_EVENT_JSON_BYTES: usize = MAX_FILE_PICK_BYTES * 4 + MAX_EVENT_ENVELOPE_BYTES;
/// Raw file ceiling a shell may hand Core in one pick (32 MiB), which also
/// fits the generic event envelope as a JSON byte array. The bound lives
/// with the shell protocol, not in Core's limits, because the protocol is
/// what a shell builds against (ADR-078); Core's full-backup limit equals
/// it, pinned by `tests/it/payload_bounds_tests.rs`.
pub const MAX_FILE_PICK_BYTES: usize = 32 * 1024 * 1024;
const MAX_EVENT_ENVELOPE_BYTES: usize = 4 * 1024;
/// Maximum object/array nesting accepted before event deserialization.
pub const MAX_EVENT_JSON_NESTING_DEPTH: usize = 16;
/// Maximum raw text or choice value accepted from a presented input.
pub const MAX_EVENT_INPUT_VALUE_BYTES: usize = 4 * 1024;
/// A hardware shell's device name becomes the default name of a device
/// joining an identity; OS model names stay well inside this.
const MAX_DEVICE_NAME_BYTES: usize = 256;
/// Transport names are short tokens Core knows ("ble", "camera").
const MAX_TRANSPORT_NAME_BYTES: usize = 64;

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
    if let Event::FilePickCompleted {
        request_id,
        outcome,
    } = event
    {
        if request_id.is_empty() || request_id.len() > 128 {
            return Err(EventJsonError::Malformed);
        }
        if let super::FilePickOutcome::Selected { bytes, filename } = outcome
            && (bytes.len() > MAX_FILE_PICK_BYTES || filename.len() > 1024)
        {
            return Err(EventJsonError::TooLarge);
        }
    }
    if let Event::HardwareCapabilitiesReported { capabilities } = event
        && capabilities
            .device_name
            .as_ref()
            .is_some_and(|name| name.len() > MAX_DEVICE_NAME_BYTES)
    {
        return Err(EventJsonError::Malformed);
    }
    if let Event::PermissionRequired { transport } | Event::PermissionGranted { transport } = event
        && transport.len() > MAX_TRANSPORT_NAME_BYTES
    {
        return Err(EventJsonError::Malformed);
    }
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
