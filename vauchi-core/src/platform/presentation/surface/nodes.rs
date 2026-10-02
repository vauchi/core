// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

use super::AccessibilitySpec;
use crate::platform::{ActionSpec, BindingId};

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PresentationAxis {
    Horizontal,
    Vertical,
}

/// How a shell draws a `List`'s rows. `Rows` is the ordinary list;
/// `Buttons` draws each activatable row as a native button, for a short
/// set of commands that must read as tappable at a glance (the exchange
/// screen's switch-camera and cancel beside the camera preview).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PresentationListStyle {
    #[default]
    Rows,
    Buttons,
}

impl PresentationListStyle {
    pub fn is_rows(&self) -> bool {
        matches!(self, Self::Rows)
    }
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PresentationTextStyle {
    Heading,
    Body,
    Caption,
    Monospace,
    Muted,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PresentationInputKind {
    Text,
    Email,
    Phone,
    Url,
    Password,
    Number,
    Search,
    Pin,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PresentationTone {
    Neutral,
    Accent,
    Success,
    Warning,
    Error,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PresentationImageShape {
    Natural,
    Circle,
}

/// Where inside its square a display code is drawn: the code's side and
/// the offset of its top-left corner, in permille of the square's side.
///
/// Built only through [`Self::new`] and deserialisation, both of which
/// keep the code inside the square and at least half its side, so a held
/// value never asks a shell to draw outside its node.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct QrPlacement {
    size: u16,
    x: u16,
    y: u16,
}

impl QrPlacement {
    /// Smallest side a placed code may have, in permille of the square.
    pub const MIN_SIZE: u16 = 500;
    const FULL: u16 = 1000;

    /// A placement, or `None` if the code would be smaller than
    /// [`Self::MIN_SIZE`] or reach outside the square.
    pub fn new(size: u16, x: u16, y: u16) -> Option<Self> {
        let fits = |offset: u16| offset <= Self::FULL - size;
        ((Self::MIN_SIZE..=Self::FULL).contains(&size) && fits(x) && fits(y)).then_some(Self {
            size,
            x,
            y,
        })
    }

    pub fn size(&self) -> u16 {
        self.size
    }

    pub fn x(&self) -> u16 {
        self.x
    }

    pub fn y(&self) -> u16 {
        self.y
    }
}

impl<'de> Deserialize<'de> for QrPlacement {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            size: u16,
            x: u16,
            y: u16,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.size, wire.x, wire.y)
            .ok_or_else(|| serde::de::Error::custom("placement outside its square"))
    }
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PresentationQrPurpose {
    Display,
    Capture,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InputValue {
    Text(String),
    Boolean(bool),
    Choice(Option<String>),
    Number(f64),
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOption {
    pub id: String,
    pub label: String,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentationPaging {
    pub total_count: usize,
    pub offset: usize,
    pub window: usize,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PresentationRow {
    pub title: String,
    pub subtitle: Option<String>,
    pub detail: Option<String>,
    pub icon_token: Option<String>,
    pub image_data: Option<Vec<u8>>,
    pub fallback_text: Option<String>,
    pub selected: bool,
    pub enabled: bool,
    pub activation: Option<ActionSpec>,
    pub secondary_actions: Vec<ActionSpec>,
    pub controls: Vec<PresentationNode>,
    pub accessibility: AccessibilitySpec,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PresentationNode {
    Text {
        id: Option<BindingId>,
        content: String,
        style: PresentationTextStyle,
        accessibility: AccessibilitySpec,
    },
    Input {
        binding_id: BindingId,
        label: String,
        value: String,
        placeholder: Option<String>,
        input_kind: PresentationInputKind,
        max_length: Option<usize>,
        validation_error: Option<String>,
        enabled: bool,
        accessibility: AccessibilitySpec,
    },
    Toggle {
        binding_id: BindingId,
        label: String,
        value: bool,
        enabled: bool,
        accessibility: AccessibilitySpec,
    },
    Choice {
        binding_id: BindingId,
        label: String,
        selected: Option<String>,
        options: Vec<ChoiceOption>,
        enabled: bool,
        accessibility: AccessibilitySpec,
    },
    Group {
        id: Option<BindingId>,
        label: Option<String>,
        axis: PresentationAxis,
        children: Vec<PresentationNode>,
        accessibility: AccessibilitySpec,
    },
    List {
        id: BindingId,
        label: Option<String>,
        rows: Vec<PresentationRow>,
        searchable: bool,
        paging: Option<PresentationPaging>,
        accessibility: AccessibilitySpec,
        /// Omitted on the wire when `Rows`, so every existing fixture and
        /// shell keeps working unchanged.
        #[serde(default, skip_serializing_if = "PresentationListStyle::is_rows")]
        style: PresentationListStyle,
    },
    Image {
        id: Option<BindingId>,
        data: Option<Vec<u8>>,
        fallback_text: Option<String>,
        shape: PresentationImageShape,
        /// Square, in logical units, the picture is fitted into with its
        /// aspect kept and never wider than the space it is given. `None`
        /// leaves the shell's own sizing (avatars size from the touch
        /// target). Absent on the wire when `None`, so shells that predate
        /// it keep decoding.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<u16>,
        brightness: f32,
        activation: Option<ActionSpec>,
        accessibility: AccessibilitySpec,
    },
    Status {
        id: Option<BindingId>,
        title: String,
        detail: Option<String>,
        icon_token: Option<String>,
        badge: Option<String>,
        tone: PresentationTone,
        activation: Option<ActionSpec>,
        accessibility: AccessibilitySpec,
    },
    Qr {
        id: BindingId,
        payloads: Vec<String>,
        purpose: PresentationQrPurpose,
        label: Option<String>,
        /// Where in the node's square to draw a display code. Absent
        /// means the full square, centred.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placement: Option<QrPlacement>,
        accessibility: AccessibilitySpec,
    },
    Confirmation {
        id: BindingId,
        warning: String,
        confirm: ActionSpec,
        cancel: ActionSpec,
        accessibility: AccessibilitySpec,
    },
    Slider {
        binding_id: BindingId,
        label: String,
        value: f64,
        minimum: f64,
        maximum: f64,
        step: Option<f64>,
        minimum_icon: Option<String>,
        maximum_icon: Option<String>,
        accessibility: AccessibilitySpec,
    },
    Progress {
        label: Option<String>,
        value: Option<f64>,
        accessibility: AccessibilitySpec,
    },
    Divider,
}
