// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

use super::component::Component;

/// Describes a full screen to render.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScreenModel {
    pub screen_id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub components: Vec<Component>,
    pub actions: Vec<ScreenAction>,
    pub progress: Option<Progress>,
}

/// Step progress indicator.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    pub current_step: u8,
    pub total_steps: u8,
    pub label: Option<String>,
}

/// A button or action the user can take on the screen.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenAction {
    pub id: String,
    pub label: String,
    pub style: ActionStyle,
    pub enabled: bool,
}

/// Visual style for a screen action.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ActionStyle {
    Primary,
    Secondary,
    Destructive,
}
