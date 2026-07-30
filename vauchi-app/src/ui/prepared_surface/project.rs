// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;

use vauchi_core::{
    AccessibilitySpec, ActionSpec, ActionTone, BindingId, InteractionId, PresentationAxis,
    PresentationInputKind, PresentationNode, PresentationTone,
};

use super::PreparedSurfaceError;
use crate::ui::{
    A11y, Field, IndicatorKind, InputType, PreviewVariant, SettingsItemKind, Status, TextStyle,
    UserAction,
};

mod collections;
mod components;
mod remaining;

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
}

impl Projection {
    pub(super) fn new(revision: u64) -> Self {
        Self {
            value_routes: HashMap::new(),
            interaction_routes: HashMap::new(),
            next_binding: 0,
            next_interaction: 0,
            revision,
        }
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
        Ok(PresentationNode::Input {
            binding_id,
            label: input.label.to_owned(),
            value: input.value.to_owned(),
            placeholder: input.placeholder,
            input_kind: input.input_kind,
            max_length: input.max_length,
            validation_error: input.validation_error,
            enabled: true,
            accessibility: accessibility(input.a11y, input.label),
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

    pub(super) fn node_id(&mut self) -> Result<BindingId, PreparedSurfaceError> {
        let id = BindingId::new(format!(
            "surface.{}.node.{}",
            self.revision, self.next_binding
        ))?;
        self.next_binding += 1;
        Ok(id)
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
