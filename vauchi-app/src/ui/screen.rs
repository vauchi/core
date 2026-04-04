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
    /// Component types scheduled for removal. Frontends rendering these
    /// should display a migration hint in debug builds. Empty unless a
    /// deprecation cycle is in progress (2 major versions before removal).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deprecated_components: Vec<String>,
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
            deprecated_components: Vec::new(),
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
            deprecated_components: Vec::new(),
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
        assert!(json.contains("\"schema_version\":1"));
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

/// Metadata for a navigation tab. Core resolves localized labels so
/// frontends never hardcode tab strings or icon names.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabInfo {
    /// Stable identifier matching `AppScreen::screen_id()`.
    pub id: String,
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
