// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core's answer to an event it refuses.
//!
//! Core owns error→consequence translation (ADR-045 Am1), so every refusal
//! comes back as a prepared alert and no shell composes copy from an error
//! value.

use vauchi_core::{AlertSpec, Command, EventJsonError, MAX_EVENT_INPUT_VALUE_BYTES};

use crate::i18n::{Locale, get_string, get_string_with_args};

/// An over-bound input value is the one refusal a user can cause (pasting a
/// whole backup into a text field), so it names the bound; the rest are shell
/// defects and get generic copy.
pub(crate) fn event_json_rejection(locale: Locale, error: &EventJsonError) -> Vec<Command> {
    let message = match error {
        EventJsonError::InputValueTooLarge => get_string_with_args(
            locale,
            "validation.too_long",
            &[("max", &MAX_EVENT_INPUT_VALUE_BYTES.to_string())],
        ),
        EventJsonError::TooLarge | EventJsonError::TooDeep | EventJsonError::Malformed => {
            tracing::warn!(kind = ?error, "event refused at the boundary decoder");
            get_string(locale, "error.generic")
        }
    };
    rejection_alert(locale, message)
}

pub(crate) fn dispatch_rejection(locale: Locale) -> Vec<Command> {
    rejection_alert(locale, get_string(locale, "error.generic"))
}

fn rejection_alert(locale: Locale, message: String) -> Vec<Command> {
    vec![Command::PresentAlert {
        alert: AlertSpec {
            title: get_string(locale, "error.title"),
            message,
        },
    }]
}
