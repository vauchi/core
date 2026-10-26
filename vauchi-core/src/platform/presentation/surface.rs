// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::{Deserialize, Serialize};

use super::SurfaceId;

mod nodes;

pub use nodes::{
    ChoiceOption, InputValue, PresentationAxis, PresentationImageShape, PresentationInputKind,
    PresentationListStyle, PresentationNode, PresentationPaging, PresentationQrErrorCorrection,
    PresentationQrPurpose, PresentationQrSize, PresentationRow, PresentationTextStyle,
    PresentationTone, QrPlacement,
};

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessibilitySpec {
    pub label: String,
    pub description: Option<String>,
}

impl AccessibilitySpec {
    pub fn label(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: None,
        }
    }
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SurfaceLayout {
    Scroll,
    Fixed,
    Pinned,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentationTokens {
    pub spacing_small: u16,
    pub spacing_medium: u16,
    pub spacing_large: u16,
    pub corner_radius: u16,
    pub minimum_target_size: u16,
    pub colors: Box<PresentationColors>,
}

/// The active theme's colours by role, as `#rrggbb`. A `None` role is one
/// the theme leaves out; the shell paints its nearest required role.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentationColors {
    pub background: String,
    pub surface: String,
    pub text: String,
    pub text_muted: String,
    pub accent: String,
    pub border: String,
    pub success: String,
    pub warning: String,
    pub error: String,
    pub surface_input: Option<String>,
    pub surface_muted: Option<String>,
    pub surface_elevated: Option<String>,
    pub text_on_accent: Option<String>,
    pub focus_ring: Option<String>,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceSpec {
    pub surface_id: SurfaceId,
    /// Monotonic Core-owned replacement generation for this surface.
    pub revision: u64,
    pub title: String,
    pub subtitle: Option<String>,
    pub accessibility_label: String,
    pub layout: SurfaceLayout,
    pub tokens: PresentationTokens,
    pub nodes: Vec<PresentationNode>,
}
