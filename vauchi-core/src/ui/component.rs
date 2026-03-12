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
    ContactList {
        id: String,
        contacts: Vec<ContactItem>,
        searchable: bool,
    },
    SettingsGroup {
        id: String,
        label: String,
        items: Vec<SettingsItem>,
    },
    ActionList {
        id: String,
        items: Vec<ActionListItem>,
    },
    StatusIndicator {
        id: String,
        icon: Option<String>,
        title: String,
        detail: Option<String>,
        status: Status,
    },
    PinInput {
        id: String,
        label: String,
        length: usize,
        filled: usize,
        masked: bool,
        validation_error: Option<String>,
    },
    QrCode {
        id: String,
        data: String,
        mode: QrMode,
        label: Option<String>,
    },
    ConfirmationDialog {
        id: String,
        title: String,
        message: String,
        confirm_text: String,
        destructive: bool,
    },
    /// A non-blocking toast message with optional undo action.
    ShowToast {
        id: String,
        message: String,
        /// If set, show an Undo button that emits UndoPressed with this action_id.
        undo_action_id: Option<String>,
        /// Auto-dismiss duration in milliseconds (default: 5000).
        duration_ms: u32,
    },
    /// An inline confirmation for irrevocable actions (expands in place).
    InlineConfirm {
        id: String,
        warning: String,
        confirm_text: String,
        cancel_text: String,
        /// If true, render confirm button in destructive/red style.
        destructive: bool,
    },
    /// A text field that toggles between display and edit mode.
    EditableText {
        id: String,
        label: String,
        value: String,
        /// When true, render as editable input. When false, render as static text with edit button.
        editing: bool,
        validation_error: Option<String>,
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
    Password,
}

/// How field visibility is controlled in the UI.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum VisibilityMode {
    /// No visibility column — display fields read-only.
    ReadOnly,
    /// Show/hide toggle per field.
    ShowHide,
    /// Per-group visibility controls.
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

/// A lightweight contact summary for list display.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactItem {
    pub id: String,
    pub name: String,
    pub subtitle: Option<String>,
    pub avatar_initials: String,
    pub status: Option<String>,
    /// Field values available for search (phone numbers, emails, etc.).
    /// Not displayed directly — used by ContactListEngine for full-text search.
    #[serde(default)]
    pub searchable_fields: Vec<String>,
}

/// An item in a settings group.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsItem {
    pub id: String,
    pub label: String,
    pub kind: SettingsItemKind,
}

/// The kind of a settings item.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingsItemKind {
    Toggle { enabled: bool },
    Value { value: String },
    Link { detail: Option<String> },
    Destructive { label: String },
}

/// An item in an action list.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionListItem {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub detail: Option<String>,
}

/// Status for a status indicator component.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Pending,
    InProgress,
    Success,
    Failed,
    Warning,
}

/// QR code display mode.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QrMode {
    Display,
    Scan,
}
