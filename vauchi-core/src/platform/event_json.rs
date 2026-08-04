// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::Event;

/// Maximum JSON payload accepted by the generic presentation dispatch path.
pub const MAX_EVENT_JSON_BYTES: usize = 64 * 1024;
/// Maximum object/array nesting accepted before event deserialization.
pub const MAX_EVENT_JSON_NESTING_DEPTH: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EventJsonError {
    #[error("event JSON exceeds {MAX_EVENT_JSON_BYTES} bytes")]
    TooLarge,
    #[error("event JSON exceeds nesting depth {MAX_EVENT_JSON_NESTING_DEPTH}")]
    TooDeep,
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

    serde_json::from_str(json).map_err(|_| EventJsonError::Malformed)
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
