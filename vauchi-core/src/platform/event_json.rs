// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::Event;

/// Maximum JSON payload accepted by the generic presentation dispatch path.
pub const MAX_EVENT_JSON_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EventJsonError {
    #[error("event JSON exceeds {MAX_EVENT_JSON_BYTES} bytes")]
    TooLarge,
    #[error("event JSON is malformed")]
    Malformed,
}

/// Parse one bounded generic event received from a presentation shell.
pub fn event_from_json(json: &str) -> Result<Event, EventJsonError> {
    if json.len() > MAX_EVENT_JSON_BYTES {
        return Err(EventJsonError::TooLarge);
    }

    serde_json::from_str(json).map_err(|_| EventJsonError::Malformed)
}
