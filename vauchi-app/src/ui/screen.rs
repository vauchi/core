// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

use super::component::{A11y, Component};
use crate::theme::DesignTokens;

/// Current schema version. Increment when adding new Component types.
/// Shells use this to detect unsupported components and degrade gracefully.
pub const CURRENT_SCHEMA_VERSION: u16 = 3;

/// How the frontend should present this screen. Replaces frontend-side
/// substring checks on `screen_id` (e.g. windows
/// `screen_id == "form_dialog"`, macOS `screen_id.hasPrefix("link_")`).
///
/// Per `2026-05-01-screen-id-metadata-in-core` G2 — the renderer routes
/// on the discriminant; it doesn't read the screen-id strings.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ScreenPresentationKind {
    /// Standard pushed/replaced page in the navigation stack.
    #[default]
    Page,
    /// Modal dialog stacked over the page (form dialogs, confirmations).
    /// Tab switches must dismiss before navigating away.
    Modal,
    /// Bottom-sheet / system-sheet style presentation. Pre-empts the
    /// iOS / macOS native sheet semantics that today live entirely in
    /// the frontend.
    Sheet,
}

/// Whether the renderer wraps content in a scroll container or renders
/// a fixed, non-scrolling layout sized to the viewport. Frontends honour
/// this instead of always wrapping content in a scroll view. `Fixed` is
/// used by screens that must not reflow while a live element updates —
/// e.g. the QR exchange screen, where a moving QR breaks the peer
/// camera lock (`2026-06-03-exchange-qr-scan-stability`). `Pinned` is
/// used by list-dominant screens: chrome stays pinned while the list
/// component owns scrolling lazily — eager rendering of 10k rows froze
/// and crashed the mobile renderers
/// (`2026-06-11-contacts-list-windowing-design`).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ScreenLayout {
    /// Content may exceed the viewport; renderer wraps it in a scroll
    /// container (the default for list / detail screens).
    #[default]
    Scroll,
    /// Content is sized to the viewport and must not scroll or reflow.
    Fixed,
    /// Chrome is sized to the viewport and does not scroll; the
    /// screen's list component is the scroll host (and, unlike
    /// `Fixed`, overlays such as sync chrome may still reflow it).
    Pinned,
}

impl ScreenLayout {
    /// `true` when this is the default scrolling layout. Used by serde
    /// `skip_serializing_if` so the field is omitted unless `Fixed` or
    /// `Pinned`.
    pub fn is_scroll(&self) -> bool {
        matches!(self, ScreenLayout::Scroll)
    }
}

/// Hint telling the native shell which hardware-wrapper flow hosts this
/// screen. Replaces frontend-side `screen_id` substring checks used to
/// decide whether to render a `CoreScreenView` or a dedicated native
/// exchange wrapper (`2026-07-06-mobile-domain-shell-violations` I5/A2).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum NativeWrapperHint {
    /// No native wrapper; render through the standard core screen renderer.
    #[default]
    None,
    /// The multi-stage bilateral QR/camera exchange wrapper.
    MultiStageExchange,
    /// The NFC tap-to-exchange wrapper.
    NfcExchange,
}

impl NativeWrapperHint {
    /// `true` when the hint is `None`. Used by serde `skip_serializing_if`
    /// so the common case (standard core rendering) stays off the wire.
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

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
    /// Component types scheduled for removal. Frontends rendering these
    /// should display a migration hint in debug builds. Empty unless a
    /// deprecation cycle is in progress (2 major versions before removal).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deprecated_components: Vec<String>,
    /// Sidebar/tab the screen belongs to, when it is a sub-screen of
    /// a top-level tab (e.g. `contact_detail` belongs to `contacts`).
    /// `None` for top-level tabs and transient/global screens (lock,
    /// onboarding). Replaces frontend-side `MapScreenToParentId`-style
    /// switch statements (`2026-05-01-screen-id-metadata-in-core` G1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_screen_id: Option<String>,
    /// How to present the screen. See [`ScreenPresentationKind`]; G2 of
    /// `2026-05-01-screen-id-metadata-in-core`.
    #[serde(default)]
    pub presentation_kind: ScreenPresentationKind,
    /// Whether this screen offers a back affordance — engine nav state
    /// (`AppEngine::can_go_back`) stamped at the render boundary, so frontends
    /// gate their system-back handler on the rendered screen instead of a
    /// separate `can_go_back()` query (ADR-043 Am4). Absent == false (most
    /// screens are roots / back-stoppers).
    #[serde(default, skip_serializing_if = "is_false")]
    pub can_go_back: bool,
    /// Whether the renderer scrolls the content or renders a fixed,
    /// non-scrolling layout. See [`ScreenLayout`]. Omitted on the wire
    /// when `Scroll` (the default) so only fixed-layout screens carry it.
    #[serde(default, skip_serializing_if = "ScreenLayout::is_scroll")]
    pub layout: ScreenLayout,
    /// Whether the screen needs an animated-QR frame-cycle timer while
    /// visible. Core owns the decision; frontends start/stop the timer based
    /// on this flag instead of matching domain `screen_id`s
    /// (`2026-07-06-mobile-domain-shell-violations` I4).
    #[serde(default, skip_serializing_if = "is_false")]
    pub requires_animated_qr: bool,
    /// Whether the screen needs a periodic poll tick while visible (e.g.
    /// the multi-stage exchange engine). Core owns the decision; frontends
    /// start/stop the poll loop based on this flag instead of matching
    /// domain `screen_id`s.
    #[serde(default, skip_serializing_if = "is_false")]
    pub requires_poll: bool,
    /// Which native hardware wrapper should host this screen, if any.
    /// `None` means render through the standard core screen renderer.
    /// Core owns the decision; frontends stop matching domain `screen_id`s
    /// to decide between `CoreScreenView` and dedicated exchange wrappers
    /// (`2026-07-06-mobile-domain-shell-violations` I5/A2).
    #[serde(default, skip_serializing_if = "NativeWrapperHint::is_none")]
    pub native_wrapper_hint: NativeWrapperHint,
    /// Global/top-bar chrome actions offered on this screen (e.g. the
    /// Settings gear on the home screen). Core owns *what* chrome actions
    /// exist; each frontend presents them per its form factor (mobile
    /// top-bar, desktop may route to its sidebar) instead of hardcoding
    /// native chrome — retires android's `ReadyScreen`/`isHomeTab` gate and
    /// the iOS `HomeView` header (`2026-07-06-mobile-domain-shell-violations`).
    /// Reserved chrome ids (e.g. `open_settings`) resolve to their
    /// `NavigateTo` before per-screen dispatch. Empty on most screens.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nav_actions: Vec<ScreenAction>,
}

/// serde `skip_serializing_if` predicate for `bool` fields defaulting to
/// `false` — keeps the wire JSON (and golden fixtures) free of the field on
/// the common case.
fn is_false(value: &bool) -> bool {
    !*value
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
            // Active tokens here too, so struct-literal builders using
            // `..Default::default()` reflect a token hot-reload. The serde
            // field default (deserialization) stays on the type default via
            // the `#[serde(default)]` on `tokens` — unaffected by this.
            tokens: crate::theme::active_design_tokens(),
            deprecated_components: Vec::new(),
            parent_screen_id: None,
            presentation_kind: ScreenPresentationKind::Page,
            can_go_back: false,
            layout: ScreenLayout::Scroll,
            requires_animated_qr: false,
            requires_poll: false,
            native_wrapper_hint: NativeWrapperHint::None,
            nav_actions: Vec::new(),
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
            // Source tokens from the hot-reload store so a reloaded
            // tokens.json shows up in every emitted screen; falls back to
            // the bundled default until a reload (ADR-038 Amendment 2).
            tokens: crate::theme::active_design_tokens(),
            deprecated_components: Vec::new(),
            parent_screen_id: None,
            presentation_kind: ScreenPresentationKind::Page,
            can_go_back: false,
            layout: ScreenLayout::Scroll,
            requires_animated_qr: false,
            requires_poll: false,
            native_wrapper_hint: NativeWrapperHint::None,
            nav_actions: Vec::new(),
        }
    }

    /// Return a copy with `validation_error` set on the component matching
    /// `component_id`. Works for `TextInput`, `PinInput`, and `EditableText`.
    ///
    /// If no matching component is found, the screen is returned unchanged.
    /// This allows `AppEngine` to convert `ActionResult::ValidationError`
    /// into `ActionResult::UpdateScreen` so frontends never need to patch
    /// the model themselves.
    pub fn with_validation_error(mut self, component_id: &str, message: String) -> Self {
        for component in &mut self.components {
            match component {
                Component::TextInput {
                    id,
                    validation_error,
                    ..
                } if id == component_id => {
                    *validation_error = Some(message);
                    return self;
                }
                Component::PinInput {
                    id,
                    validation_error,
                    ..
                } if id == component_id => {
                    *validation_error = Some(message);
                    return self;
                }
                Component::EditableText {
                    id,
                    validation_error,
                    ..
                } if id == component_id => {
                    *validation_error = Some(message);
                    return self;
                }
                _ => {}
            }
        }
        self
    }
}

// INLINE_TEST_REQUIRED: backward-compat + schema_version tests
#[cfg(test)]
mod tests {
    use super::*;

    /// Old JSON (pre-schema_version) must still parse. The serde
    /// defaults fill schema_version and tokens automatically.
    #[test]
    fn legacy_json_without_new_fields_parses() {
        let legacy = r#"{
            "screen_id": "test",
            "title": "Test Screen",
            "subtitle": null,
            "components": [],
            "actions": [],
            "progress": null
        }"#;
        let m: ScreenModel = serde_json::from_str(legacy).expect("legacy JSON must parse");
        assert_eq!(m.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(m.tokens, DesignTokens::default());
        assert!(m.deprecated_components.is_empty());
    }

    #[test]
    fn screen_model_json_includes_schema_version() {
        let m = ScreenModel::new("test", "Title", vec![], vec![]);
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains(&format!("\"schema_version\":{CURRENT_SCHEMA_VERSION}")));
    }

    // @internal
    #[test]
    fn pinned_layout_round_trips_on_the_wire() {
        let mut m = ScreenModel::new("contacts", "Contacts", vec![], vec![]);
        m.layout = ScreenLayout::Pinned;
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"layout\":\"Pinned\""));
        let back: ScreenModel = serde_json::from_str(&json).unwrap();
        assert_eq!(back.layout, ScreenLayout::Pinned);
    }

    #[test]
    fn screen_model_json_includes_tokens() {
        let m = ScreenModel::new("test", "Title", vec![], vec![]);
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"spacing\""));
        assert!(json.contains("\"border_radius\""));
        assert!(json.contains("\"md_lg\":12"));
    }

    #[test]
    fn deprecated_components_omitted_when_empty() {
        let m = ScreenModel::new("test", "Title", vec![], vec![]);
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            !json.contains("deprecated_components"),
            "empty deprecated_components must be omitted: {json}"
        );
    }

    #[test]
    fn deprecated_components_roundtrips() {
        let mut m = ScreenModel::new("test", "Title", vec![], vec![]);
        m.deprecated_components = vec!["OldWidget".to_string()];
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"deprecated_components\""));
        let restored: ScreenModel = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.deprecated_components, vec!["OldWidget"]);
    }

    // G1 of `2026-05-01-screen-id-metadata-in-core` — `parent_screen_id`
    // surfaces the sidebar tab a sub-screen belongs to. Default is `None`
    // (top-level screens are their own parent — frontends select the tab
    // matching `screen_id`). Frontends use the new field to highlight the
    // parent tab when rendering a detail screen, replacing per-frontend
    // hardcoded `screenId` switch-statements (windows
    // `MapScreenToParentId`, linux-qt `screenrenderer.cpp:200` /
    // `device_linking`, macOS `DeviceLinkSheet:113` `link_` prefix check).

    // @internal
    #[test]
    fn parent_screen_id_defaults_to_none() {
        let m = ScreenModel::new("test", "Title", vec![], vec![]);
        assert_eq!(m.parent_screen_id, None);
    }

    // @internal
    #[test]
    fn parent_screen_id_omitted_when_none() {
        let m = ScreenModel::new("test", "Title", vec![], vec![]);
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            !json.contains("parent_screen_id"),
            "None parent_screen_id must be omitted from wire JSON: {json}"
        );
    }

    // @internal
    #[test]
    fn parent_screen_id_roundtrips_when_set() {
        let mut m = ScreenModel::new("contact_detail", "Alice", vec![], vec![]);
        m.parent_screen_id = Some("contacts".into());
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"parent_screen_id\":\"contacts\""));
        let restored: ScreenModel = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.parent_screen_id, Some("contacts".into()));
    }

    // G2 of the same record — `presentation_kind` distinguishes Page /
    // Modal / Sheet without forcing frontends to substring-match
    // `screen_id`. Defaults to Page (regular pushed/replaced screens);
    // form dialogs flip to Modal; bottom-sheet-style screens flip to
    // Sheet (pre-empting the iOS/macOS native sheet semantics that today
    // live entirely in the frontend).

    // @internal
    #[test]
    fn presentation_kind_defaults_to_page() {
        let m = ScreenModel::new("test", "Title", vec![], vec![]);
        assert_eq!(m.presentation_kind, ScreenPresentationKind::Page);
    }

    // @internal
    #[test]
    fn presentation_kind_serializes() {
        let mut m = ScreenModel::new("form_dialog", "Pick a group", vec![], vec![]);
        m.presentation_kind = ScreenPresentationKind::Modal;
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            json.contains("\"presentation_kind\":\"Modal\""),
            "presentation_kind must serialize as the variant name: {json}"
        );
    }

    // @internal
    #[test]
    fn presentation_kind_roundtrips_each_variant() {
        for kind in [
            ScreenPresentationKind::Page,
            ScreenPresentationKind::Modal,
            ScreenPresentationKind::Sheet,
        ] {
            let mut m = ScreenModel::new("test", "Title", vec![], vec![]);
            m.presentation_kind = kind.clone();
            let json = serde_json::to_string(&m).unwrap();
            let restored: ScreenModel = serde_json::from_str(&json).unwrap();
            assert_eq!(
                restored.presentation_kind, kind,
                "presentation_kind round-trip failed for {kind:?}"
            );
        }
    }

    // @internal
    #[test]
    fn legacy_json_without_presentation_kind_parses_as_page() {
        // Existing in-flight ScreenModel JSON predating these fields must
        // still parse — frontends pinned to old core revs may emit it.
        let legacy = r#"{
            "screen_id": "test",
            "title": "Test Screen",
            "subtitle": null,
            "components": [],
            "actions": [],
            "progress": null
        }"#;
        let m: ScreenModel = serde_json::from_str(legacy).expect("legacy JSON must parse");
        assert_eq!(m.presentation_kind, ScreenPresentationKind::Page);
        assert_eq!(m.parent_screen_id, None);
    }

    // Lifecycle hints for hardware-timer ownership (I4 of
    // `2026-07-06-mobile-domain-shell-violations`). Core tells the shell
    // which screens need animated-QR frame cycling or poll ticks, so
    // frontends stop branching on domain `screen_id`s.

    // @internal
    #[test]
    fn lifecycle_hints_default_to_false() {
        let m = ScreenModel::new("test", "Title", vec![], vec![]);
        assert!(
            !m.requires_animated_qr,
            "requires_animated_qr must default to false"
        );
        assert!(!m.requires_poll, "requires_poll must default to false");
    }

    // @internal
    #[test]
    fn lifecycle_hints_roundtrip_on_the_wire() {
        let mut m = ScreenModel::new("test", "Title", vec![], vec![]);
        m.requires_animated_qr = true;
        m.requires_poll = true;
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            json.contains("\"requires_animated_qr\":true"),
            "requires_animated_qr must serialize when true: {json}"
        );
        assert!(
            json.contains("\"requires_poll\":true"),
            "requires_poll must serialize when true: {json}"
        );
        let restored: ScreenModel = serde_json::from_str(&json).unwrap();
        assert!(restored.requires_animated_qr);
        assert!(restored.requires_poll);
    }

    // @internal
    #[test]
    fn lifecycle_hints_omitted_when_false() {
        let m = ScreenModel::new("test", "Title", vec![], vec![]);
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            !json.contains("requires_animated_qr"),
            "false requires_animated_qr must be omitted from wire JSON: {json}"
        );
        assert!(
            !json.contains("requires_poll"),
            "false requires_poll must be omitted from wire JSON: {json}"
        );
    }

    // @internal
    #[test]
    fn legacy_json_without_lifecycle_hints_parses_as_false() {
        let legacy = r#"{
            "screen_id": "test",
            "title": "Test Screen",
            "subtitle": null,
            "components": [],
            "actions": [],
            "progress": null
        }"#;
        let m: ScreenModel = serde_json::from_str(legacy).expect("legacy JSON must parse");
        assert!(!m.requires_animated_qr);
        assert!(!m.requires_poll);
    }

    // Native wrapper hints (I5/A2 of
    // `2026-07-06-mobile-domain-shell-violations`). Core tells the shell
    // which screens render inside a dedicated hardware wrapper instead of
    // the standard core renderer, so frontends stop matching domain
    // `screen_id`s like `multi_stage_exchange` / `exchange_nfc*`.

    // @internal
    #[test]
    fn native_wrapper_hint_defaults_to_none() {
        let m = ScreenModel::new("test", "Title", vec![], vec![]);
        assert_eq!(m.native_wrapper_hint, NativeWrapperHint::None);
    }

    // @internal
    #[test]
    fn native_wrapper_hint_omitted_when_none() {
        let m = ScreenModel::new("test", "Title", vec![], vec![]);
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            !json.contains("native_wrapper_hint"),
            "None native_wrapper_hint must be omitted from wire JSON: {json}"
        );
    }

    // @internal
    #[test]
    fn native_wrapper_hint_roundtrips_each_variant() {
        for hint in [
            NativeWrapperHint::MultiStageExchange,
            NativeWrapperHint::NfcExchange,
        ] {
            let mut m = ScreenModel::new("test", "Title", vec![], vec![]);
            m.native_wrapper_hint = hint.clone();
            let json = serde_json::to_string(&m).unwrap();
            let restored: ScreenModel = serde_json::from_str(&json).unwrap();
            assert_eq!(
                restored.native_wrapper_hint, hint,
                "native_wrapper_hint round-trip failed for {hint:?}"
            );
        }
    }

    // @internal
    #[test]
    fn legacy_json_without_native_wrapper_hint_parses_as_none() {
        let legacy = r#"{
            "screen_id": "test",
            "title": "Test Screen",
            "subtitle": null,
            "components": [],
            "actions": [],
            "progress": null
        }"#;
        let m: ScreenModel = serde_json::from_str(legacy).expect("legacy JSON must parse");
        assert_eq!(m.native_wrapper_hint, NativeWrapperHint::None);
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
///
/// Frontends render this as the primary button row on a screen.
/// The `id` is the stable action identifier consumed by
/// `handle_action` via `UserAction::ActionPressed { action_id }`;
/// the `label` is the localized display string.
///
/// # Accessibility contract
///
/// - `id` is the stable accessibility identifier (Compose
///   `testTag`, SwiftUI `accessibilityIdentifier`). Frontends
///   map `ScreenAction.id` onto the platform a11y identifier
///   so Maestro (or any a11y-based test driver) can tap by
///   `id:` rather than by localized visible text. When `a11y`
///   is `None`, `label` serves as the screen-reader
///   announcement.
/// - `a11y.label`, when `Some`, overrides `label` for screen
///   readers. Use this for destructive buttons
///   ("Delete permanently, this cannot be undone") or
///   toggles-as-buttons ("Turned on" vs the visible "On").
///   For 95%+ of screens, leave `a11y` as `None`.
///
/// See `_private/docs/problems/2026-04-20-screen-action-a11y-identifier-gap`.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenAction {
    pub id: String,
    pub label: String,
    pub style: ActionStyle,
    pub enabled: bool,
    /// Accessibility override. `None` means "use `label`".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a11y: Option<A11y>,
}

/// Metadata for a navigation tab. Core resolves localized labels so
/// frontends never hardcode tab strings or icon names.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabInfo {
    /// Stable identifier matching `AppScreen::screen_id()`. Used by frontends
    /// only for selection equality against `current_tab_id` — never to
    /// construct a navigation target.
    pub id: String,
    /// Opaque navigation token. Frontends forward it verbatim via
    /// `UserAction::NavigateToTab { action_id }` on tap; core resolves it to
    /// `NavigateTo`. The frontend never parses or branches on it
    /// (ADR-043 Amendment 4 / ADR-044 Wire Humble).
    pub action_id: String,
    /// Localized display label (resolved by core from `nav.*` keys).
    pub label: String,
    /// Icon name (SF Symbol format). Frontends map to platform equivalents
    /// (e.g., Material Icons on Android, SF Symbols on iOS).
    pub icon: String,
    /// Badge count (e.g., pending contact updates). Zero means no badge.
    #[serde(default)]
    pub badge_count: u32,
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
