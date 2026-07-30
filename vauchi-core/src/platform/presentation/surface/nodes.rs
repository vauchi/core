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
    },
    Image {
        id: Option<BindingId>,
        data: Option<Vec<u8>>,
        fallback_text: Option<String>,
        shape: PresentationImageShape,
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
