// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared JSON transport helpers for mobile/platform FFI modules.
//!
//! These functions convert between core UI types and JSON strings for
//! the UniFFI boundary. Both `mobile_ui` and `platform_app_engine` use them.

use serde::Serialize;
use vauchi_app::ui::{ActionResult, AppScreen, ScreenModel, UserAction};
use vauchi_core::Command;

use crate::error::MobileError;

/// Serialize a `ScreenModel` to JSON.
pub(crate) fn screen_to_json(screen: &ScreenModel) -> Result<String, MobileError> {
    serde_json::to_string(screen).map_err(|e| MobileError::Other {
        detail: format!("Failed to serialize ScreenModel: {e}"),
    })
}

/// Serialize an `ActionResult` to JSON.
pub(crate) fn action_result_to_json(result: &ActionResult) -> Result<String, MobileError> {
    serde_json::to_string(result).map_err(|e| MobileError::Other {
        detail: format!("Failed to serialize ActionResult: {e}"),
    })
}

/// Envelope returned by `navigate_to_json` / `navigate_back_json`. Carries
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

/// Deserialize a `UserAction` from JSON.
pub(crate) fn user_action_from_json(json: &str) -> Result<UserAction, MobileError> {
    serde_json::from_str(json).map_err(|e| MobileError::InvalidInput {
        field: String::new(),
        detail: format!("Failed to parse UserAction JSON: {e}"),
    })
}

/// Deserialize an `AppScreen` from JSON.
pub(crate) fn app_screen_from_json(json: &str) -> Result<AppScreen, MobileError> {
    serde_json::from_str(json).map_err(|e| MobileError::InvalidInput {
        field: String::new(),
        detail: format!("Failed to parse AppScreen JSON: {e}"),
    })
}
