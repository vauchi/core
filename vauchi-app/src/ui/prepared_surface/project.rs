// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;

use vauchi_core::{
    AccessibilitySpec, ActionSpec, ActionTone, BindingId, InteractionId, PresentationAxis,
    PresentationInputKind, PresentationNode, PresentationTone,
};

use super::PreparedSurfaceError;
use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::{
    A11y, Field, IndicatorKind, InputType, PreviewVariant, SettingsItemKind, Status, TextStyle,
    UserAction,
};

mod collections;
mod components;
mod remaining;

/// Prefix of a capture node's binding id; no revision follows it.
const CAPTURE_BINDING_PREFIX: &str = "surface.capture.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ValueRoute {
    Text {
        component_id: String,
    },
    ToggleItem {
        component_id: String,
        item_id: String,
    },
    SettingsToggle {
        component_id: String,
        item_id: String,
    },
    Choice {
        component_id: String,
    },
    Variant,
    Slider {
        component_id: String,
    },
    FieldVisibility {
        field_id: String,
        group_id: Option<String>,
    },
}

pub(super) struct InputProjection<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub value: &'a str,
    pub placeholder: Option<String>,
    pub max_length: Option<usize>,
    pub validation_error: Option<String>,
    pub input_kind: PresentationInputKind,
    pub a11y: &'a Option<A11y>,
}

pub(super) struct PreviewProjection<'a> {
    pub name: &'a str,
    pub initials: &'a str,
    pub image_data: &'a Option<Vec<u8>>,
    pub variants: &'a [PreviewVariant],
    pub selected_variant: &'a Option<String>,
    pub visible_fields: &'a [Field],
    pub a11y: &'a Option<A11y>,
}

pub(super) struct Projection {
    pub(super) value_routes: HashMap<BindingId, ValueRoute>,
    pub(super) interaction_routes: HashMap<InteractionId, UserAction>,
    next_binding: u64,
    next_interaction: u64,
    revision: u64,
    locale: Locale,
}

impl Projection {
    pub(super) fn new(revision: u64, locale: Locale) -> Self {
        Self {
            value_routes: HashMap::new(),
            interaction_routes: HashMap::new(),
            next_binding: 0,
            next_interaction: 0,
            revision,
            locale,
        }
    }

    /// The info action of an item that names an `info_key`: labelled
    /// "Info", named "About {name}" for assistive tech, routed to the key
    /// (vauchi/private#479). `None` for an item without one.
    pub(super) fn item_info(
        &mut self,
        info_key: Option<&str>,
        name: &str,
    ) -> Result<Option<ActionSpec>, PreparedSurfaceError> {
        let Some(key) = info_key else {
            return Ok(None);
        };
        let label = get_string(self.locale, "context_bar.info");
        let spoken =
            get_string_with_args(self.locale, "context_bar.info_item_a11y", &[("name", name)]);
        self.action(
            &label,
            AccessibilitySpec::label(&spoken),
            ActionTone::Standard,
            UserAction::InfoRequested {
                key: key.to_owned(),
            },
        )
        .map(Some)
    }

    fn input(
        &mut self,
        input: InputProjection<'_>,
    ) -> Result<PresentationNode, PreparedSurfaceError> {
        let binding_id = self.qualified_id(input.id)?;
        self.value_routes.insert(
            binding_id.clone(),
            ValueRoute::Text {
                component_id: input.id.to_owned(),
            },
        );
        let prepared = accessibility(input.a11y, input.label);
        Ok(PresentationNode::Input {
            binding_id,
            label: input.label.to_owned(),
            value: input.value.to_owned(),
            placeholder: input.placeholder,
            input_kind: input.input_kind,
            max_length: input.max_length,
            enabled: true,
            // A rejected field must announce why. Only GTK4 offers an
            // error-message relation; Qt has no public equivalent and
            // replacing QAccessibleLineEdit to add one would cost the field
            // its text interface. So Core puts the reason where every shell
            // already maps it — the accessible description, which outranks
            // the usage hint while the value is rejected
            // (problems/2026-08-21-linux-shells-drop-core-a11y).
            accessibility: AccessibilitySpec {
                description: input.validation_error.clone().or(prepared.description),
                ..prepared
            },
            validation_error: input.validation_error,
        })
    }

    pub(super) fn binding(&mut self, route: ValueRoute) -> Result<BindingId, PreparedSurfaceError> {
        let id = BindingId::new(format!(
            "surface.{}.binding.{}",
            self.revision, self.next_binding
        ))?;
        self.next_binding += 1;
        self.value_routes.insert(id.clone(), route);
        Ok(id)
    }

    /// The binding of a camera's capture node. Unlike every other id it
    /// carries no revision: the camera is the same node for as long as it
    /// is on screen, and it reads on its own clock, so a decode is often
    /// reported after the surface moved on by a frame. Re-minting it per
    /// revision made those decodes stale (about half of them at a 100 ms
    /// frame, rig runs E7, 2026-10-02).
    pub(super) fn capture_binding(
        &mut self,
        component_id: &str,
    ) -> Result<BindingId, PreparedSurfaceError> {
        let id = BindingId::new(format!("{CAPTURE_BINDING_PREFIX}{component_id}"))?;
        self.value_routes.insert(
            id.clone(),
            ValueRoute::Text {
                component_id: component_id.to_owned(),
            },
        );
        Ok(id)
    }

    /// Whether `id` has the form of a capture binding.
    pub(super) fn is_capture_binding(id: &str) -> bool {
        id.starts_with(CAPTURE_BINDING_PREFIX)
    }

    pub(super) fn node_id(&mut self) -> Result<BindingId, PreparedSurfaceError> {
        let id = BindingId::new(format!(
            "surface.{}.node.{}",
            self.revision, self.next_binding
        ))?;
        self.next_binding += 1;
        Ok(id)
    }

    /// The revision a Core-minted id was projected at, read back from the
    /// `surface.{revision}.` prefix every id above carries.
    pub(super) fn minted_revision(id: &str) -> Option<u64> {
        id.strip_prefix("surface.")?.split('.').next()?.parse().ok()
    }

    pub(super) fn qualified_id(&self, id: &str) -> Result<BindingId, PreparedSurfaceError> {
        Ok(BindingId::new(format!("surface.{}.{}", self.revision, id))?)
    }

    pub(super) fn action(
        &mut self,
        label: &str,
        accessibility: AccessibilitySpec,
        tone: ActionTone,
        route: UserAction,
    ) -> Result<ActionSpec, PreparedSurfaceError> {
        let interaction_id = InteractionId::new(format!(
            "surface.{}.interaction.{}",
            self.revision, self.next_interaction
        ))?;
        self.next_interaction += 1;
        self.interaction_routes
            .insert(interaction_id.clone(), route);
        Ok(ActionSpec {
            interaction_id,
            label: label.to_owned(),
            accessibility_label: accessibility.label,
            icon_token: None,
            enabled: true,
            tone,
            shortcut: None,
        })
    }
}

pub(super) fn accessibility(a11y: &Option<A11y>, fallback: &str) -> AccessibilitySpec {
    AccessibilitySpec {
        label: a11y
            .as_ref()
            .and_then(|metadata| metadata.label.clone())
            .unwrap_or_else(|| fallback.to_owned()),
        description: a11y.as_ref().and_then(|metadata| metadata.hint.clone()),
    }
}

fn group(
    id: Option<BindingId>,
    label: Option<String>,
    children: Vec<PresentationNode>,
    accessibility: AccessibilitySpec,
) -> PresentationNode {
    PresentationNode::Group {
        id,
        label,
        axis: PresentationAxis::Vertical,
        children,
        accessibility,
    }
}

fn text_style(style: &TextStyle) -> vauchi_core::PresentationTextStyle {
    match style {
        TextStyle::Title => vauchi_core::PresentationTextStyle::Heading,
        TextStyle::Subtitle => vauchi_core::PresentationTextStyle::Muted,
        TextStyle::Body => vauchi_core::PresentationTextStyle::Body,
        TextStyle::Caption => vauchi_core::PresentationTextStyle::Caption,
    }
}

fn input_kind(input_type: &InputType) -> PresentationInputKind {
    match input_type {
        InputType::Text => PresentationInputKind::Text,
        InputType::Phone => PresentationInputKind::Phone,
        InputType::Email => PresentationInputKind::Email,
        InputType::Password => PresentationInputKind::Password,
    }
}

fn status_tone(status: Status) -> PresentationTone {
    match status {
        Status::Pending => PresentationTone::Neutral,
        Status::InProgress => PresentationTone::Accent,
        Status::Success => PresentationTone::Success,
        Status::Failed => PresentationTone::Error,
        Status::Warning => PresentationTone::Warning,
    }
}

pub(super) fn indicator_tone(kind: IndicatorKind) -> PresentationTone {
    match kind {
        IndicatorKind::Active => PresentationTone::Success,
        IndicatorKind::Error => PresentationTone::Error,
        IndicatorKind::Neutral => PresentationTone::Neutral,
        IndicatorKind::Busy => PresentationTone::Accent,
    }
}

pub(super) fn setting_is_destructive(kind: &SettingsItemKind) -> bool {
    matches!(kind, SettingsItemKind::Destructive { .. })
}
