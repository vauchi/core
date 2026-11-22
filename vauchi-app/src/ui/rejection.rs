// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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

/// An event's variant, never its fields: a value may be what the person
/// typed (logging rules). For the log line of a refused event
/// (vauchi/private#1339).
pub(crate) fn event_kind(event: &vauchi_core::Event) -> String {
    match serde_json::to_value(event) {
        Ok(serde_json::Value::String(unit)) => unit,
        Ok(serde_json::Value::Object(fields)) => fields
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "unknown".to_owned()),
        _ => "unknown".to_owned(),
    }
}

/// Why `dispatch` refused, one level deeper than
/// `AppPresentationError::kind` where the prepared surface said why.
pub(crate) fn rejection_detail(error: &crate::ui::AppPresentationError) -> String {
    use crate::ui::{AppPresentationError, PreparedSurfaceError};
    match error {
        AppPresentationError::Prepared(reason) => format!(
            "prepared.{}",
            match reason {
                PreparedSurfaceError::InvalidIdentifier(_) => "invalid_identifier",
                PreparedSurfaceError::UnsupportedComponent => "unsupported_component",
                PreparedSurfaceError::SurfaceMismatch => "surface_mismatch",
                PreparedSurfaceError::UnknownBinding => "unknown_binding",
                PreparedSurfaceError::ValueTypeMismatch => "value_type_mismatch",
                PreparedSurfaceError::UnsupportedEvent => "unsupported_event",
            }
        ),
        other => other.kind().to_owned(),
    }
}

// INLINE_TEST_REQUIRED: tests exercise the crate-private diagnostics helpers
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::AppPresentationError;
    use crate::ui::PreparedSurfaceError;
    use vauchi_core::Event;
    use vauchi_core::platform::{BindingId, SurfaceId};

    /// A rejected event is logged by its variant only: the variant says
    /// which event a shell sent too late or wrongly, its fields may hold
    /// what the person typed (vauchi/private#1339).
    // @internal
    #[test]
    fn a_rejected_event_is_named_by_its_variant_and_nothing_else() {
        let typed = Event::ValueChanged {
            surface_id: SurfaceId::new("onboarding").unwrap(),
            binding_id: BindingId::new("display_name").unwrap(),
            value: vauchi_core::platform::InputValue::Text("Carol".into()),
        };
        assert_eq!(event_kind(&typed), "ValueChanged");
        assert_eq!(event_kind(&Event::AppBackgrounded), "AppBackgrounded");
        assert!(!event_kind(&typed).contains("Carol"));
    }

    /// "prepared" alone did not say whether the binding was unknown, the
    /// surface another one, or the value of the wrong type.
    // @internal
    #[test]
    fn a_prepared_surface_rejection_names_its_reason() {
        assert_eq!(
            rejection_detail(&AppPresentationError::Prepared(
                PreparedSurfaceError::UnknownBinding
            )),
            "prepared.unknown_binding"
        );
        assert_eq!(
            rejection_detail(&AppPresentationError::Prepared(
                PreparedSurfaceError::SurfaceMismatch
            )),
            "prepared.surface_mismatch"
        );
        assert_eq!(
            rejection_detail(&AppPresentationError::InvalidBackgroundBudget),
            "invalid_background_budget"
        );
    }
}
