// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{
    AccessibilitySpec, ActionTone, PresentationInputKind, PresentationNode, PresentationRow,
    PresentationTextStyle, PresentationTone,
};

use super::{Projection, ValueRoute, accessibility, group, indicator_tone};
use crate::ui::{
    A11y, Component, Field, PreparedSurfaceError, UiFieldVisibility, UserAction, VisibilityMode,
};

mod interaction_nodes;

impl Projection {
    pub(super) fn remaining_component(
        &mut self,
        component: &Component,
    ) -> Result<PresentationNode, PreparedSurfaceError> {
        match component {
            Component::FieldList {
                id,
                title,
                fields,
                visibility_mode,
                available_scopes,
                a11y,
            } => self.field_list(id, title, fields, visibility_mode, available_scopes, a11y),
            Component::InlineConfirm {
                id,
                warning,
                confirm_text,
                cancel_text,
                confirm_action_id,
                cancel_action_id,
                destructive,
                a11y,
            } => Ok(PresentationNode::Confirmation {
                id: vauchi_core::BindingId::new(id)?,
                warning: warning.clone(),
                confirm: self.action(
                    confirm_text,
                    AccessibilitySpec::label(confirm_text),
                    if *destructive {
                        ActionTone::Destructive
                    } else {
                        ActionTone::Standard
                    },
                    UserAction::ActionPressed {
                        action_id: confirm_action_id.clone(),
                    },
                )?,
                cancel: self.action(
                    cancel_text,
                    AccessibilitySpec::label(cancel_text),
                    ActionTone::Standard,
                    UserAction::ActionPressed {
                        action_id: cancel_action_id.clone(),
                    },
                )?,
                accessibility: accessibility(a11y, warning),
            }),
            Component::EditableText {
                id,
                label,
                value,
                edit_text,
                save_text,
                cancel_text,
                edit_action_id,
                save_action_id,
                cancel_action_id,
                editing,
                validation_error,
                a11y,
                ..
            } => self.editable_text(
                id,
                label,
                value,
                edit_text,
                save_text,
                cancel_text,
                edit_action_id,
                save_action_id,
                cancel_action_id,
                *editing,
                validation_error,
                a11y,
            ),
            Component::Banner {
                text,
                action_label,
                action_id,
                a11y,
            } => Ok(PresentationNode::Status {
                id: None,
                title: text.clone(),
                detail: None,
                icon_token: None,
                badge: None,
                tone: PresentationTone::Accent,
                // A banner is one widget that is both the content and the
                // affordance, so it gets one prepared name. Shells cannot
                // surface two: neither AT-SPI toolkit lets an application set
                // an action description, and naming the widget after the verb
                // hides what it is about
                // (problems/2026-08-21-linux-shells-drop-core-a11y).
                activation: Some(self.action(
                    action_label,
                    accessibility(a11y, text),
                    ActionTone::Standard,
                    UserAction::ActionPressed {
                        action_id: action_id.clone(),
                    },
                )?),
                accessibility: accessibility(a11y, text),
            }),
            Component::Slider {
                id,
                label,
                value,
                min,
                max,
                step,
                min_icon,
                max_icon,
                a11y,
            } => {
                let binding_id = self.binding(ValueRoute::Slider {
                    component_id: id.clone(),
                })?;
                Ok(PresentationNode::Slider {
                    binding_id,
                    label: label.clone(),
                    value: f64::from(*value),
                    minimum: f64::from(*min),
                    maximum: f64::from(*max),
                    step: (*step > 0.0).then(|| f64::from(*step)),
                    minimum_icon: min_icon.clone(),
                    maximum_icon: max_icon.clone(),
                    accessibility: accessibility(a11y, label),
                })
            }
            Component::Indicator {
                id,
                label,
                kind,
                action_id,
                a11y,
            } => Ok(PresentationNode::Status {
                id: Some(vauchi_core::BindingId::new(id)?),
                title: label.clone(),
                detail: None,
                icon_token: None,
                badge: None,
                tone: indicator_tone(*kind),
                activation: action_id
                    .as_ref()
                    .map(|action_id| {
                        self.action(
                            label,
                            accessibility(a11y, label),
                            ActionTone::Standard,
                            UserAction::ActionPressed {
                                action_id: action_id.clone(),
                            },
                        )
                    })
                    .transpose()?,
                accessibility: accessibility(a11y, label),
            }),
            _ => Err(PreparedSurfaceError::UnsupportedComponent),
        }
    }

    fn field_list(
        &mut self,
        id: &str,
        title: &str,
        fields: &[Field],
        mode: &VisibilityMode,
        scopes: &[String],
        a11y: &Option<A11y>,
    ) -> Result<PresentationNode, PreparedSurfaceError> {
        let mut rows = Vec::with_capacity(fields.len());
        for field in fields {
            rows.push(PresentationRow {
                title: field.label.clone(),
                subtitle: Some(field.value.clone()),
                detail: Some(field.visibility_label.clone()),
                icon_token: Some(field.icon.clone()),
                image_data: None,
                fallback_text: None,
                selected: matches!(field.visibility, UiFieldVisibility::Shown),
                enabled: true,
                activation: None,
                secondary_actions: Vec::new(),
                controls: self.visibility_controls(field, mode, scopes)?,
                accessibility: accessibility(&field.a11y, &field.label),
            });
        }
        Ok(PresentationNode::List {
            id: vauchi_core::BindingId::new(id)?,
            label: Some(title.to_owned()),
            rows,
            searchable: false,
            paging: None,
            accessibility: accessibility(a11y, title),
        })
    }

    fn visibility_controls(
        &mut self,
        field: &Field,
        mode: &VisibilityMode,
        scopes: &[String],
    ) -> Result<Vec<PresentationNode>, PreparedSurfaceError> {
        match mode {
            VisibilityMode::ReadOnly => Ok(Vec::new()),
            VisibilityMode::ShowHide => Ok(vec![self.visibility_toggle(
                field,
                None,
                matches!(field.visibility, UiFieldVisibility::Shown),
            )?]),
            VisibilityMode::PerGroup => scopes
                .iter()
                .map(|scope| {
                    let visible = matches!(
                        &field.visibility,
                        UiFieldVisibility::Scopes(selected) if selected.contains(scope)
                    );
                    self.visibility_toggle(field, Some(scope.clone()), visible)
                })
                .collect(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn editable_text(
        &mut self,
        id: &str,
        label: &str,
        value: &str,
        edit_text: &str,
        save_text: &str,
        cancel_text: &str,
        edit_action_id: &str,
        save_action_id: &str,
        cancel_action_id: &str,
        editing: bool,
        validation_error: &Option<String>,
        a11y: &Option<A11y>,
    ) -> Result<PresentationNode, PreparedSurfaceError> {
        let mut children = Vec::new();
        if editing {
            children.push(self.input(super::InputProjection {
                id,
                label,
                value,
                placeholder: None,
                max_length: None,
                validation_error: validation_error.clone(),
                input_kind: PresentationInputKind::Text,
                a11y,
            })?);
            for (text, action_id) in [(save_text, save_action_id), (cancel_text, cancel_action_id)]
            {
                children.push(self.action_node(text, action_id)?);
            }
        } else {
            children.push(PresentationNode::Text {
                id: Some(vauchi_core::BindingId::new(id)?),
                content: value.to_owned(),
                style: PresentationTextStyle::Body,
                accessibility: accessibility(a11y, label),
            });
            children.push(self.action_node(edit_text, edit_action_id)?);
        }
        Ok(group(
            Some(vauchi_core::BindingId::new(id)?),
            Some(label.to_owned()),
            children,
            accessibility(a11y, label),
        ))
    }
}
