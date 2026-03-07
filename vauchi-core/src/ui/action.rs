// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

use super::screen::ScreenModel;

/// An action the user performed in the UI.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum UserAction {
    TextChanged {
        component_id: String,
        value: String,
    },
    ItemToggled {
        component_id: String,
        item_id: String,
    },
    ActionPressed {
        action_id: String,
    },
    FieldVisibilityChanged {
        field_id: String,
        group_id: Option<String>,
        visible: bool,
    },
    GroupViewSelected {
        group_name: Option<String>,
    },
}

/// The result of handling a user action.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ActionResult {
    UpdateScreen(ScreenModel),
    NavigateTo(ScreenModel),
    ValidationError {
        component_id: String,
        message: String,
    },
    Complete,
}
