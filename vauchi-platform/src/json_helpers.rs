// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared JSON transport helpers for mobile/platform FFI modules.
//!
//! These functions convert between core UI types and JSON strings for
//! the UniFFI boundary. Both `mobile_ui` and `platform_app_engine` use them.

use vauchi_app::ui::{ActionResult, AppScreen, ScreenModel, UserAction};

use crate::error::MobileError;

/// Serialize a `ScreenModel` to JSON.
pub(crate) fn screen_to_json(screen: &ScreenModel) -> Result<String, MobileError> {
    serde_json::to_string(screen)
        .map_err(|e| MobileError::Internal(format!("Failed to serialize ScreenModel: {e}")))
}

/// Serialize an `ActionResult` to JSON.
pub(crate) fn action_result_to_json(result: &ActionResult) -> Result<String, MobileError> {
    serde_json::to_string(result)
        .map_err(|e| MobileError::Internal(format!("Failed to serialize ActionResult: {e}")))
}

/// Deserialize a `UserAction` from JSON.
pub(crate) fn user_action_from_json(json: &str) -> Result<UserAction, MobileError> {
    serde_json::from_str(json)
        .map_err(|e| MobileError::InvalidInput(format!("Failed to parse UserAction JSON: {e}")))
}

/// Deserialize an `AppScreen` from JSON.
pub(crate) fn app_screen_from_json(json: &str) -> Result<AppScreen, MobileError> {
    serde_json::from_str(json)
        .map_err(|e| MobileError::InvalidInput(format!("Failed to parse AppScreen JSON: {e}")))
}
