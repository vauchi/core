// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Deserializer, Serialize};

const MAX_PRESENTATION_ID_LENGTH: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PresentationIdError {
    #[error("presentation identifier must not be empty")]
    Empty,
    #[error("presentation identifier exceeds 128 bytes")]
    TooLong,
    #[error("presentation identifier contains a control character")]
    ControlCharacter,
}

fn validate_identifier(value: &str) -> Result<(), PresentationIdError> {
    if value.is_empty() {
        return Err(PresentationIdError::Empty);
    }
    if value.len() > MAX_PRESENTATION_ID_LENGTH {
        return Err(PresentationIdError::TooLong);
    }
    if value.chars().any(char::is_control) {
        return Err(PresentationIdError::ControlCharacter);
    }
    Ok(())
}

macro_rules! presentation_identifier {
    ($name:ident) => {
        #[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, PresentationIdError> {
                let value = value.into();
                validate_identifier(&value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

presentation_identifier!(SurfaceId);
presentation_identifier!(InteractionId);
presentation_identifier!(BindingId);

mod effects;
mod surface;

pub use effects::{AlertSpec, ExportFileSpec, NotificationSpec, NotificationUrgency, ToastSpec};
pub use surface::{
    AccessibilitySpec, ChoiceOption, InputValue, PresentationAxis, PresentationImageShape,
    PresentationInputKind, PresentationNode, PresentationPaging, PresentationQrPurpose,
    PresentationRow, PresentationTextStyle, PresentationTokens, PresentationTone, SurfaceLayout,
    SurfaceSpec,
};

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StandardShortcut {
    Back,
    ActivatePrimary,
    Undo,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionSpec {
    pub interaction_id: InteractionId,
    pub label: String,
    pub accessibility_label: String,
    pub icon_token: Option<String>,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "ActionTone::is_standard")]
    pub tone: ActionTone,
    pub shortcut: Option<StandardShortcut>,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ActionTone {
    #[default]
    Standard,
    Destructive,
}

impl ActionTone {
    fn is_standard(&self) -> bool {
        matches!(self, Self::Standard)
    }
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBar {
    pub back: Option<ActionSpec>,
    pub navigation: Option<ActionSpec>,
    pub primary: Option<ActionSpec>,
    pub secondary: Option<ActionSpec>,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OverlayKind {
    Navigation,
    ActionMenu,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlaySpec {
    pub kind: OverlayKind,
    pub title: Option<String>,
    pub items: Vec<ActionSpec>,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InputMode {
    Touch,
    Pointer,
    Keyboard,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MotionPreference {
    Full,
    Reduced,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum WindowClass {
    Compact,
    Medium,
    Expanded,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PaneLayout {
    Single,
    Split,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentationProfile {
    pub window_class: WindowClass,
    pub pane_layout: PaneLayout,
    pub primary_surface: SurfaceId,
    pub detail_surface: Option<SurfaceId>,
    pub active_surface: SurfaceId,
}
