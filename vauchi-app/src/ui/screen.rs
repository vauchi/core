// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

use super::component::Component;
use crate::theme::DesignTokens;

/// Current schema version. Increment when adding new Component types.
/// Shells use this to detect unsupported components and degrade gracefully.
pub const CURRENT_SCHEMA_VERSION: u16 = 1;

/// Describes a full screen to render.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScreenModel {
    /// Schema version — shells ignore components from higher versions.
    #[serde(default = "default_schema_version")]
    pub schema_version: u16,
    pub screen_id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub components: Vec<Component>,
    pub actions: Vec<ScreenAction>,
    pub progress: Option<Progress>,
    /// Resolved design tokens for layout consistency. Frontends read
    /// spacing, radius, typography from here — never hardcode values.
    #[serde(default)]
    pub tokens: DesignTokens,
}

fn default_schema_version() -> u16 {
    CURRENT_SCHEMA_VERSION
}

impl Default for ScreenModel {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            screen_id: String::new(),
            title: String::new(),
            subtitle: None,
            components: Vec::new(),
            actions: Vec::new(),
            progress: None,
            tokens: DesignTokens::default(),
        }
    }
}

impl ScreenModel {
    /// Create a ScreenModel with schema_version and default tokens pre-filled.
    pub fn new(
        screen_id: impl Into<String>,
        title: impl Into<String>,
        components: Vec<Component>,
        actions: Vec<ScreenAction>,
    ) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            screen_id: screen_id.into(),
            title: title.into(),
            subtitle: None,
            components,
            actions,
            progress: None,
            tokens: DesignTokens::default(),
        }
    }
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
