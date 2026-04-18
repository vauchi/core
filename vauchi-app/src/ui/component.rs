// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

/// An option in a [`Component::Dropdown`].
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DropdownOption {
    pub id: String,
    pub label: String,
}

/// A UI component that core tells frontends to render.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
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
        #[serde(default)]
        a11y: Option<A11y>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        info_key: Option<String>,
    },
    ToggleList {
        id: String,
        label: String,
        items: Vec<ToggleItem>,
        #[serde(default)]
        a11y: Option<A11y>,
    },
    FieldList {
        id: String,
        fields: Vec<FieldDisplay>,
        visibility_mode: VisibilityMode,
        available_groups: Vec<String>,
        #[serde(default)]
        a11y: Option<A11y>,
    },
    CardPreview {
        name: String,
        /// Avatar image bytes (WebP). Frontends show this in the circular
        /// header area. Falls back to initials from `name` when None.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        avatar_data: Option<Vec<u8>>,
        fields: Vec<FieldDisplay>,
        group_views: Vec<GroupCardView>,
        selected_group: Option<String>,
        #[serde(default)]
        a11y: Option<A11y>,
    },
    InfoPanel {
        id: String,
        icon: Option<String>,
        title: String,
        items: Vec<InfoItem>,
        #[serde(default)]
        a11y: Option<A11y>,
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
        #[serde(default)]
        a11y: Option<A11y>,
    },
    PinInput {
        id: String,
        label: String,
        length: usize,
        filled: usize,
        masked: bool,
        validation_error: Option<String>,
        #[serde(default)]
        a11y: Option<A11y>,
    },
    QrCode {
        id: String,
        data: String,
        mode: QrMode,
        label: Option<String>,
        /// Real-time scan quality for the viewfinder frame indicator.
        /// Only meaningful when `mode` is `Scan`; `None` when `Display`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scan_quality: Option<ScanQuality>,
        #[serde(default)]
        a11y: Option<A11y>,
    },
    /// An inline confirmation for irrevocable actions (expands in place).
    InlineConfirm {
        id: String,
        warning: String,
        confirm_text: String,
        cancel_text: String,
        /// If true, render confirm button in destructive/red style.
        destructive: bool,
        #[serde(default)]
        a11y: Option<A11y>,
    },
    /// A text field that toggles between display and edit mode.
    EditableText {
        id: String,
        label: String,
        value: String,
        /// When true, render as editable input. When false, render as static text with edit button.
        editing: bool,
        validation_error: Option<String>,
        #[serde(default)]
        a11y: Option<A11y>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        info_key: Option<String>,
    },
    Divider,
    /// Informational banner with an optional action button (e.g. preview mode indicator).
    Banner {
        text: String,
        action_label: String,
        action_id: String,
        #[serde(default)]
        a11y: Option<A11y>,
    },
    /// An inline dropdown for selection UIs (e.g. theme, language).
    /// Reuses `UserAction::ListItemSelected` — no new action variant needed.
    Dropdown {
        id: String,
        label: String,
        selected: Option<String>,
        options: Vec<DropdownOption>,
        #[serde(default)]
        a11y: Option<A11y>,
    },
    /// Circular avatar preview with optional brightness adjustment.
    ///
    /// Used in the Avatar Editor screen and as a display component.
    /// When `editable` is true, tapping emits
    /// `UserAction::ActionPressed { action_id: "edit_avatar" }`.
    AvatarPreview {
        id: String,
        /// Raw image bytes (WebP/PNG/JPEG) to display, or None for
        /// initials-only fallback.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        image_data: Option<Vec<u8>>,
        /// Fallback initials text when no image is available.
        initials: String,
        /// Background color for initials fallback `[r, g, b]`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bg_color: Option<[u8; 3]>,
        /// Brightness adjustment (-0.3 to 0.3). 0.0 = no change.
        #[serde(default)]
        brightness: f32,
        /// Whether tapping opens the avatar editor (own card only).
        #[serde(default)]
        editable: bool,
        #[serde(default)]
        a11y: Option<A11y>,
    },
    /// A range slider for continuous value input.
    ///
    /// Emits `UserAction::SliderChanged { component_id, value }` on
    /// value changes.
    Slider {
        id: String,
        label: String,
        /// Current value.
        value: f32,
        /// Minimum allowed value.
        min: f32,
        /// Maximum allowed value.
        max: f32,
        /// Step increment (0.0 = continuous).
        #[serde(default)]
        step: f32,
        /// Optional icon name for the min end (e.g., "sun.min").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_icon: Option<String>,
        /// Optional icon name for the max end (e.g., "sun.max").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_icon: Option<String>,
        #[serde(default)]
        a11y: Option<A11y>,
    },
}

/// Semantic role hint for screen readers.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AccessibilityRole {
    Button,
    Link,
    Heading,
    Image,
    Toggle,
    TextField,
    Alert,
}

/// Accessibility metadata for a UI component.
///
/// Populated by core engines so frontends can apply consistent a11y
/// attributes without inventing their own labels.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct A11y {
    /// Screen-reader label (maps to contentDescription/accessibilityLabel).
    pub label: Option<String>,
    /// Usage hint (maps to accessibilityHint/stateDescription).
    pub hint: Option<String>,
    /// Semantic role for screen readers.
    #[serde(default)]
    pub role: Option<AccessibilityRole>,
}

/// Text rendering style.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextStyle {
    Title,
    Subtitle,
    Body,
    Caption,
}

/// Input field type hint for frontends.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputType {
    Text,
    Phone,
    Email,
    Password,
}

/// How field visibility is controlled in the UI.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
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
    #[serde(default)]
    pub a11y: Option<A11y>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_key: Option<String>,
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
    #[serde(default)]
    pub a11y: Option<A11y>,
}

/// UI-level field visibility state.
///
/// Named `UiFieldVisibility` to distinguish from `contact::FieldVisibility`
/// which is the storage-level visibility model.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
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
    #[serde(default)]
    pub a11y: Option<A11y>,
}

/// An item in a settings group.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsItem {
    pub id: String,
    pub label: String,
    pub kind: SettingsItemKind,
    #[serde(default)]
    pub a11y: Option<A11y>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_key: Option<String>,
}

/// The kind of a settings item.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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
    #[serde(default)]
    pub a11y: Option<A11y>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info_key: Option<String>,
}

/// Status for a status indicator component.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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
#[non_exhaustive]
pub enum QrMode {
    Display,
    Scan,
}

/// Real-time scan quality indicator for QR camera viewfinder.
///
/// Frontends render this as a colored border/frame around the camera
/// preview to guide the user's device positioning:
/// - `Good` → green frame (QR reliably detected)
/// - `Weak` → yellow frame (QR detected but low confidence)
/// - `Poor` → orange frame (intermittent detection)
/// - `NoSignal` → red frame (nothing detected / wrong pointing)
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ScanQuality {
    Good,
    Weak,
    Poor,
    NoSignal,
}
