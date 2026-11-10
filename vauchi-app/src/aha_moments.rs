// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Localized aha moment helpers.
//!
//! Extends `vauchi_core::AhaMomentType` with i18n-aware title and message.
//! English title()/message() remain on `AhaMomentType` in vauchi-core;
//! these functions add locale-aware variants using the i18n system.

use crate::i18n::{Locale, get_string};
use vauchi_core::{AhaMoment, AhaMomentType};

/// Get the localized title for an aha moment type.
///
/// Falls back to the hardcoded English title if no i18n key is found.
pub fn aha_moment_title_localized(moment: AhaMomentType, locale: Locale) -> String {
    let key = match moment {
        AhaMomentType::CardCreationComplete => "aha.card_creation_complete.title",
        AhaMomentType::FirstEdit => "aha.first_edit.title",
        AhaMomentType::FirstContactAdded => "aha.first_contact_added.title",
        AhaMomentType::FirstUpdateReceived => "aha.first_update_received.title",
        AhaMomentType::FirstOutboundDelivered => "aha.first_outbound_sent.title",
        AhaMomentType::FirstFieldEdit => "aha.first_field_edit.title",
        AhaMomentType::ThreeContactsReached => "aha.three_contacts_reached.title",
        AhaMomentType::DeviceLinked => "aha.device_linked.title",
        _ => "aha.unknown.title",
    };
    let s = get_string(locale, key);
    // get_string returns the key itself when no translation is found;
    // fall back to the hardcoded English title in that case.
    if s == key {
        moment.title().to_string()
    } else {
        s
    }
}

/// Get the localized message for an aha moment type.
///
/// Falls back to the hardcoded English message if no i18n key is found.
fn message_key(moment: AhaMomentType) -> &'static str {
    match moment {
        AhaMomentType::CardCreationComplete => "aha.card_creation_complete.message",
        AhaMomentType::FirstEdit => "aha.first_edit.message",
        AhaMomentType::FirstContactAdded => "aha.first_contact_added.message",
        AhaMomentType::FirstUpdateReceived => "aha.first_update_received.message",
        AhaMomentType::FirstOutboundDelivered => "aha.first_outbound_sent.message",
        AhaMomentType::FirstFieldEdit => "aha.first_field_edit.message",
        AhaMomentType::ThreeContactsReached => "aha.three_contacts_reached.message",
        AhaMomentType::DeviceLinked => "aha.device_linked.message",
        _ => "aha.unknown.message",
    }
}

pub fn aha_moment_message_localized(moment: AhaMomentType, locale: Locale) -> String {
    let key = message_key(moment);
    let s = get_string(locale, key);
    if s == key {
        moment.message().to_string()
    } else {
        s
    }
}

/// The one-line toast a shell shows for a milestone: title, then the
/// message, personalized when the locale has a `_with_name` variant for
/// the moment's context.
pub fn aha_moment_toast_localized(moment: &AhaMoment, locale: Locale) -> String {
    let title = aha_moment_title_localized(moment.moment_type, locale);
    let personalized = moment.context.as_deref().and_then(|name| {
        let key = format!("{}_with_name", message_key(moment.moment_type));
        crate::i18n::try_get_string(locale, &key).map(|s| s.replace("{name}", name))
    });
    let message =
        personalized.unwrap_or_else(|| aha_moment_message_localized(moment.moment_type, locale));
    format!("{title} {message}")
}
