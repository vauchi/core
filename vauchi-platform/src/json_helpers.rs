// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared JSON transport helpers for mobile/platform FFI modules.
//!
//! These functions convert between core UI types and JSON strings for
//! the UniFFI boundary. Both `mobile_ui` and `platform_app_engine` use them.

use serde::Serialize;
use vauchi_app::ui::{ActionResult, AppScreen, ScreenModel, UserAction};
use vauchi_core::{Command, Event};

use crate::error::MobileError;
use crate::types::notification::MobilePendingNotification;

/// Serialize a `ScreenModel` to JSON.
pub(crate) fn screen_to_json(screen: &ScreenModel) -> Result<String, MobileError> {
    serde_json::to_string(screen).map_err(|e| MobileError::Other {
        detail: format!("Failed to serialize ScreenModel: {e}"),
    })
}

/// Envelope returned by `navigate_back_json` (and the `navigate_to_json_for_test`
/// seam). Carries
/// the rendered `ScreenModel` plus any `Command`s emitted by the
/// `WorkflowEngine`'s `screen_entered` / `screen_exited` lifecycle hooks
/// during the navigation. Phase 2b of
/// `2026-05-04-exchange-command-screen-presentation`.
#[derive(Serialize)]
struct ScreenEnvelope<'a> {
    screen: &'a ScreenModel,
    commands: &'a [Command],
}

/// Envelope returned by `handle_action_json`. Carries the engine's
/// `ActionResult` plus any `Command`s emitted as a side-effect of
/// navigation during the action (e.g. an action that ends with
/// `Complete` and routes to a new screen will fire that screen's
/// `screen_entered` hook).
#[derive(Serialize)]
struct ActionResultEnvelope<'a> {
    action_result: &'a ActionResult,
    commands: &'a [Command],
}

/// Serialize a `ScreenModel` + accompanying `Command`s into the
/// navigation envelope JSON shape `{"screen": ..., "commands": [...]}`.
pub(crate) fn screen_envelope_to_json(
    screen: &ScreenModel,
    commands: &[Command],
) -> Result<String, MobileError> {
    let envelope = ScreenEnvelope { screen, commands };
    serde_json::to_string(&envelope).map_err(|e| MobileError::Other {
        detail: format!("Failed to serialize ScreenEnvelope: {e}"),
    })
}

/// Serialize an `ActionResult` + lifecycle-emitted `Command`s into the
/// action-result envelope JSON shape
/// `{"action_result": ..., "commands": [...]}`.
pub(crate) fn action_result_envelope_to_json(
    result: &ActionResult,
    commands: &[Command],
) -> Result<String, MobileError> {
    let envelope = ActionResultEnvelope {
        action_result: result,
        commands,
    };
    serde_json::to_string(&envelope).map_err(|e| MobileError::Other {
        detail: format!("Failed to serialize ActionResultEnvelope: {e}"),
    })
}

/// Envelope returned by `PlatformAppEngine::on_wakeup`. Carries the OS
/// notifications produced by the wakeup tick plus any `Command`s emitted —
/// in practice the next `Command::ScheduleWakeup` so the shell can re-arm
/// the platform scheduler (ADR-044 Am2a Option C).
#[derive(Serialize)]
struct WakeupEnvelope<'a> {
    notifications: &'a [MobilePendingNotification],
    commands: &'a [Command],
}

/// Serialize the OS notifications + drained `Command`s from `on_wakeup` into
/// the wakeup envelope JSON shape `{"notifications": [...], "commands": [...]}`.
pub(crate) fn wakeup_envelope_to_json(
    notifications: &[MobilePendingNotification],
    commands: &[Command],
) -> Result<String, MobileError> {
    let envelope = WakeupEnvelope {
        notifications,
        commands,
    };
    serde_json::to_string(&envelope).map_err(|e| MobileError::Other {
        detail: format!("Failed to serialize WakeupEnvelope: {e}"),
    })
}

#[derive(Serialize)]
struct CommandsEnvelope<'a> {
    commands: &'a [Command],
}

/// Serialize generic Core-owned presentation commands.
pub(crate) fn commands_envelope_to_json(commands: &[Command]) -> Result<String, MobileError> {
    serde_json::to_string(&CommandsEnvelope { commands }).map_err(|e| MobileError::Other {
        detail: format!("Failed to serialize CommandsEnvelope: {e}"),
    })
}

/// Deserialize a generic platform event without a hand-maintained FFI mirror.
pub(crate) fn event_from_json(json: &str) -> Result<Event, MobileError> {
    serde_json::from_str(json).map_err(|e| MobileError::InvalidInput {
        field: String::new(),
        detail: format!("Failed to parse Event JSON: {e}"),
    })
}

/// Deserialize a `UserAction` from JSON.
pub(crate) fn user_action_from_json(json: &str) -> Result<UserAction, MobileError> {
    serde_json::from_str(json).map_err(|e| MobileError::InvalidInput {
        field: String::new(),
        detail: format!("Failed to parse UserAction JSON: {e}"),
    })
}

/// Deserialize an `AppScreen` from JSON.
///
/// Accepts two forms:
/// 1. The serde `AppScreen` shape — a variant name (`"Contacts"`) or a
///    tagged object (`{"ContactDetail": {"contact_id": "…"}}`). Used for
///    parameterized screens.
/// 2. A canonical screen-id string the frontend received from core
///    (`tab_info.id` / `ScreenModel.screen_id`), e.g. `"contacts"`. Lets
///    frontends navigate by the opaque id core handed them instead of
///    constructing the serde variant name (ADR-043 Am4 — zero domain
///    vocabulary in frontends). Only simple (non-parameterized) screens
///    resolve this way.
pub(crate) fn app_screen_from_json(json: &str) -> Result<AppScreen, MobileError> {
    if let Ok(screen) = serde_json::from_str::<AppScreen>(json) {
        return Ok(screen);
    }
    if let Ok(id) = serde_json::from_str::<String>(json)
        && let Some(screen) = AppScreen::from_screen_id(&id)
    {
        return Ok(screen);
    }
    Err(MobileError::InvalidInput {
        field: String::new(),
        detail: format!("Failed to parse AppScreen JSON: {json}"),
    })
}

// INLINE_TEST_REQUIRED: app_screen_from_json is pub(crate), cannot be tested from external tests/
#[cfg(test)]
mod tests {
    use super::*;

    // Regression: the serde `AppScreen` form (variant name + tagged
    // object) still parses — frontends use it for parameterized screens.
    // @internal
    #[test]
    fn app_screen_from_json_accepts_serde_variant() {
        assert_eq!(
            app_screen_from_json("\"Contacts\"").unwrap(),
            AppScreen::Contacts
        );
        assert_eq!(
            app_screen_from_json("{\"ContactDetail\":{\"contact_id\":\"abc\"}}").unwrap(),
            AppScreen::ContactDetail {
                contact_id: "abc".to_string()
            }
        );
    }

    // ADR-043 Am4: a frontend navigates by the opaque canonical screen-id
    // core handed it (`tab_info.id` / `ScreenModel.screen_id`) — e.g.
    // "contacts" — never by constructing the serde variant name
    // ("Contacts"). The canonical-id fallback resolves these.
    // @internal
    #[test]
    fn app_screen_from_json_accepts_canonical_screen_id() {
        assert_eq!(
            app_screen_from_json("\"contacts\"").unwrap(),
            AppScreen::Contacts
        );
        assert_eq!(
            app_screen_from_json("\"my_info\"").unwrap(),
            AppScreen::MyInfo
        );
        assert_eq!(app_screen_from_json("\"more\"").unwrap(), AppScreen::More);
    }

    // @internal
    #[test]
    fn app_screen_from_json_rejects_unknown_id() {
        assert!(app_screen_from_json("\"not_a_real_screen\"").is_err());
    }
}
