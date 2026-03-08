// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

/// A UI component that core tells frontends to render.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Component {
    Text {
        id: String,
        content: String,
        style: TextStyle,
    },
    TextInput {
        id: String,
        label: String,
        value: String,
        placeholder: Option<String>,
        max_length: Option<usize>,
        validation_error: Option<String>,
        input_type: InputType,
    },
    ToggleList {
        id: String,
        label: String,
        items: Vec<ToggleItem>,
    },
    FieldList {
        id: String,
        fields: Vec<FieldDisplay>,
        visibility_mode: VisibilityMode,
        available_groups: Vec<String>,
    },
    CardPreview {
        name: String,
        fields: Vec<FieldDisplay>,
        group_views: Vec<GroupCardView>,
        selected_group: Option<String>,
    },
    InfoPanel {
        id: String,
        icon: Option<String>,
        title: String,
        items: Vec<InfoItem>,
    },
    Divider,
}

/// Text rendering style.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TextStyle {
    Title,
    Subtitle,
    Body,
    Caption,
}

/// Input field type hint for frontends.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum InputType {
    Text,
    Phone,
    Email,
}

/// How field visibility is controlled in the UI.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum VisibilityMode {
    ShowHide,
    PerGroup,
}

/// A toggleable item in a list.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToggleItem {
    pub id: String,
    pub label: String,
    pub selected: bool,
    pub subtitle: Option<String>,
}

/// A contact field as displayed in the UI.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldDisplay {
    pub id: String,
    pub field_type: String,
    pub label: String,
    pub value: String,
    pub visibility: UiFieldVisibility,
}

/// UI-level field visibility state.
///
/// Named `UiFieldVisibility` to distinguish from `contact::FieldVisibility`
/// which is the storage-level visibility model.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum UiFieldVisibility {
    Shown,
    Hidden,
    Groups(Vec<String>),
}

/// How a card looks to a specific group.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupCardView {
    pub group_name: String,
    pub display_name: String,
    pub visible_fields: Vec<FieldDisplay>,
}

/// An item in an info panel.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfoItem {
    pub icon: Option<String>,
    pub title: String,
    pub detail: String,
}
