// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

mod list;
mod preview;

pub use list::{Item, ListItemAction, ListItemActionKind};
pub(crate) use preview::initials;
pub use preview::{
    Field, PreviewVariant, UiFieldVisibility, build_visible_fields, icon_for_field_type,
};

/// Serde skip-helper: windowing fields are omitted from the wire when
/// zero so unwindowed lists keep the exact pre-windowing JSON shape.
fn is_zero(n: &usize) -> bool {
    *n == 0
}

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
        fields: Vec<Field>,
        visibility_mode: VisibilityMode,
        available_groups: Vec<String>,
        #[serde(default)]
        a11y: Option<A11y>,
    },
    Preview {
        name: String,
        /// Core-derived avatar initials (first letters of the first two
        /// words of `name`, uppercased). Frontends render this directly in
        /// the initials fallback — never recompute `name.take(1)`
        /// (ADR-021/043 Humble UI).
        initials: String,
        /// Avatar image bytes (WebP). Frontends show this in the circular
        /// header area. Falls back to `initials` when None.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        avatar_data: Option<Vec<u8>>,
        /// All fields on the card (raw — retained for backwards compatibility).
        ///
        /// Frontends should render `visible_fields` instead. This field is
        /// kept additive for one binding cycle so consumers can migrate
        /// without an ABI break; a follow-up MR removes it.
        fields: Vec<Field>,
        variants: Vec<PreviewVariant>,
        selected_variant: Option<String>,
        /// Pre-filtered fields to render — what the user actually sees.
        ///
        /// Computed by [`build_visible_fields`]: when `selected_variant` is set
        /// and matches a `PreviewVariant`, returns that group's
        /// `visible_fields`. Otherwise returns `fields` filtered to keep only
        /// `Shown` and `Groups` visibility variants. Frontends render this
        /// list directly — no `.filter` over `fields` should appear in view
        /// code (ADR-021/043 Humble UI).
        #[serde(default)]
        visible_fields: Vec<Field>,
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
    /// A list of items rendered in a uniform row layout, with optional
    /// search affordance. Frontends bind on `items`; the renderer doesn't
    /// know what kind of thing is in the list (per ADR-021/043 + Wire
    /// Humble). Engines that want to render lists of any domain emit this
    /// variant with their data mapped to `Vec<Item>`.
    List {
        id: String,
        items: Vec<Item>,
        searchable: bool,
        /// Size of the full filtered set when the emission is windowed;
        /// zero/absent = unwindowed (`items` is the complete set, the
        /// exact pre-windowing wire shape). Windowing keeps multi-MB
        /// emissions off the wire at 10k items
        /// (`2026-06-11-contacts-list-eager-render-anr` Track B).
        #[serde(default, skip_serializing_if = "is_zero")]
        total_count: usize,
        /// Window start within the filtered set (windowed emissions only).
        #[serde(default, skip_serializing_if = "is_zero")]
        offset: usize,
        /// Emitted window length. The renderer dispatches
        /// [`crate::ui::UserAction::ListWindowRequested`] as scrolling
        /// approaches the loaded window's edge.
        #[serde(default, skip_serializing_if = "is_zero")]
        window: usize,
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
    /// A horizontal container — lays its child components out in a single
    /// row. Used to place a camera preview beside its action buttons so a
    /// fixed-layout screen fits the viewport without scrolling
    /// (`2026-06-03-exchange-qr-scan-stability`). The first child flexes;
    /// later children take their natural width.
    Row {
        id: String,
        items: Vec<Component>,
    },
    StatusIndicator {
        id: String,
        icon: Option<String>,
        title: String,
        detail: Option<String>,
        status: Status,
        /// Core-resolved localized badge label for `status`. Frontends
        /// render it verbatim — deriving text from the discriminant is
        /// the W-class leak this field retires
        /// (`2026-07-06-mobile-domain-shell-violations`). Serde default
        /// keeps pre-field payloads deserializable.
        #[serde(default)]
        status_label: String,
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
    /// Ongoing-status indicator carrying a terse label, a semantic
    /// kind (color category), and an optional tap action.
    ///
    /// Distinct semantic role from `Component::StatusIndicator`,
    /// which today renders screen-body status of in-progress
    /// operations (Sync / BackupRecovery / LinkResponder /
    /// RecoveryClaimReview / RecoveryHelp use sites). `Indicator`
    /// is the chrome-positioned counterpart — emitted by AppEngine
    /// overlays (offline / update / future sync-chrome) for app-level
    /// status that lives across screens, not as screen content.
    ///
    /// Per the shell-purity investigation
    /// (`_private/docs/investigations/2026-05-28-core-screen-composition-surface.md`):
    /// the variant is generic, not Sync-specific — same shape carries
    /// connectivity / backup-overdue / update-available / sync-state
    /// uses. Frontends render natively (iOS toolbar chip, GTK4 header
    /// status icon, Android Material chip) by typed dispatch, no
    /// action_id sniffing.
    Indicator {
        id: String,
        label: String,
        kind: IndicatorKind,
        /// Optional tap action. `None` = display-only (informational);
        /// `Some(id)` = tap fires `UserAction::ActionPressed { action_id: id }`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action_id: Option<String>,
        #[serde(default)]
        a11y: Option<A11y>,
    },
    /// Sectioned action list — multiple labeled groups of tappable
    /// items, each rendered as a native section (SwiftUI `Section`,
    /// GTK4 ListBox group, Material category header). Distinct
    /// semantic role from flat `Component::ActionList`: ignoring
    /// the section grouping degrades UX from "structured menu" to
    /// "flat dump", so the discriminant belongs at variant level.
    ///
    /// Used by `MoreEngine` to emit grouped settings entries
    /// (primary / secondary / data / legal) without forcing
    /// each frontend's renderer to special-case action_ids or
    /// reproduce a section table.
    SectionedActionList {
        id: String,
        sections: Vec<Section>,
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

impl A11y {
    /// Label-only a11y — the common case where a component's visible
    /// text also serves as its screen-reader label, with no separate
    /// hint or role (M3 S5).
    pub fn labeled(label: String) -> Self {
        Self {
            label: Some(label),
            hint: None,
            role: None,
        }
    }
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

/// An item in an info panel.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfoItem {
    pub icon: Option<String>,
    pub title: String,
    pub detail: String,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Status {
    Pending,
    InProgress,
    Success,
    Failed,
    Warning,
}

impl Status {
    /// Catalog key for the badge label. Construction sites resolve it
    /// (`status_label: t(status.label_key())`) so frontends render the
    /// label verbatim and never derive text from the discriminant.
    pub fn label_key(self) -> &'static str {
        match self {
            Status::Pending => "status.pending",
            Status::InProgress => "status.in_progress",
            Status::Success => "status.success",
            Status::Failed => "status.failed",
            Status::Warning => "status.warning",
        }
    }
}

/// Semantic kind for `Component::Indicator` — the four-state color
/// category each frontend maps to its theme palette.
///
/// Distinct from `Status` (used by `Component::StatusIndicator` for
/// in-progress screen-body operations). `IndicatorKind` is for
/// ongoing-state chrome and uses presentation-shaped categories
/// (semantic color roles), not domain-shaped lifecycle states.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum IndicatorKind {
    /// In-progress or freshly-confirmed — emphasis color (e.g. green / accent).
    Active,
    /// Failed / attention-required — error color (e.g. red / orange).
    Error,
    /// Idle / informational — muted color (e.g. gray / outline).
    Neutral,
    /// Transient busy state — animated indicator (e.g. spinner / pulse).
    Busy,
}

/// A named section within a `Component::SectionedActionList`.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub id: String,
    pub label: String,
    pub items: Vec<ActionListItem>,
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
