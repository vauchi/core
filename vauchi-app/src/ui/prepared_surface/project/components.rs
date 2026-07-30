// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{
    AccessibilitySpec, ActionTone, BindingId, ChoiceOption, PresentationAxis,
    PresentationImageShape, PresentationInputKind, PresentationNode, PresentationQrPurpose,
    PresentationTone,
};

use super::{
    InputProjection, PreviewProjection, Projection, ValueRoute, accessibility, group, input_kind,
    status_tone, text_style,
};
use crate::ui::{Component, PreparedSurfaceError, QrMode, UserAction};

impl Projection {
    pub(in crate::ui::prepared_surface) fn component(
        &mut self,
        component: &Component,
    ) -> Result<PresentationNode, PreparedSurfaceError> {
        match component {
            Component::Text { id, content, style } => Ok(PresentationNode::Text {
                id: Some(BindingId::new(id)?),
                content: content.clone(),
                style: text_style(style),
                accessibility: AccessibilitySpec::label(content),
            }),
            Component::TextInput {
                id,
                label,
                value,
                placeholder,
                max_length,
                validation_error,
                input_type,
                a11y,
                ..
            } => self.input(InputProjection {
                id,
                label,
                value,
                placeholder: placeholder.clone(),
                max_length: *max_length,
                validation_error: validation_error.clone(),
                input_kind: input_kind(input_type),
                a11y,
            }),
            Component::ToggleList {
                id,
                label,
                items,
                a11y,
            } => {
                let mut children = Vec::with_capacity(items.len());
                for item in items {
                    let binding_id = self.binding(ValueRoute::ToggleItem {
                        component_id: id.clone(),
                        item_id: item.id.clone(),
                    })?;
                    children.push(PresentationNode::Toggle {
                        binding_id,
                        label: item.label.clone(),
                        value: item.selected,
                        enabled: true,
                        accessibility: accessibility(&item.a11y, &item.label),
                    });
                }
                Ok(group(
                    Some(BindingId::new(id)?),
                    Some(label.clone()),
                    children,
                    accessibility(a11y, label),
                ))
            }
            Component::InfoPanel {
                id,
                icon: _,
                title,
                items,
                a11y,
            } => {
                let children = items
                    .iter()
                    .map(|item| PresentationNode::Status {
                        id: None,
                        title: item.title.clone(),
                        detail: Some(item.detail.clone()),
                        icon_token: item.icon.clone(),
                        badge: None,
                        tone: PresentationTone::Neutral,
                        activation: None,
                        accessibility: AccessibilitySpec::label(&item.title),
                    })
                    .collect();
                let label = if title.is_empty() {
                    accessibility(a11y, id).label
                } else {
                    title.clone()
                };
                Ok(group(
                    Some(BindingId::new(id)?),
                    Some(label.clone()),
                    children,
                    accessibility(a11y, &label),
                ))
            }
            Component::List {
                id,
                items,
                searchable,
                total_count,
                offset,
                window,
            } => self.list(id, items, *searchable, *total_count, *offset, *window),
            Component::SettingsGroup { id, label, items } => self.settings_group(id, label, items),
            Component::ActionList { id, items } => self.action_list(id, None, items),
            Component::SectionedActionList { id, sections } => {
                let mut children = Vec::with_capacity(sections.len());
                for section in sections {
                    children.push(self.action_list(
                        &format!("{id}.{}", section.id),
                        Some(section.label.clone()),
                        &section.items,
                    )?);
                }
                Ok(group(
                    Some(BindingId::new(id)?),
                    None,
                    children,
                    AccessibilitySpec::label(id),
                ))
            }
            Component::Row { id, items } => {
                let mut children = Vec::with_capacity(items.len());
                for item in items {
                    children.push(self.component(item)?);
                }
                Ok(PresentationNode::Group {
                    id: Some(BindingId::new(id)?),
                    label: None,
                    axis: PresentationAxis::Horizontal,
                    children,
                    accessibility: AccessibilitySpec::label(id),
                })
            }
            Component::StatusIndicator {
                id,
                icon,
                title,
                detail,
                status,
                status_label,
                a11y,
            } => Ok(PresentationNode::Status {
                id: Some(BindingId::new(id)?),
                title: title.clone(),
                detail: detail.clone(),
                icon_token: icon.clone(),
                badge: Some(status_label.clone()),
                tone: status_tone(*status),
                activation: None,
                accessibility: accessibility(a11y, title),
            }),
            Component::PinInput {
                id,
                label,
                filled,
                length,
                masked: _,
                validation_error,
                a11y,
            } => self.input(InputProjection {
                id,
                label,
                value: &"•".repeat(*filled),
                placeholder: None,
                max_length: Some(*length),
                validation_error: validation_error.clone(),
                input_kind: PresentationInputKind::Pin,
                a11y,
            }),
            Component::QrCode {
                id,
                data,
                frames,
                mode,
                label,
                a11y,
                ..
            } => Ok(PresentationNode::Qr {
                id: if matches!(mode, QrMode::Scan) {
                    self.binding(ValueRoute::Text {
                        component_id: id.clone(),
                    })?
                } else {
                    self.qualified_id(id)?
                },
                payloads: if frames.is_empty() {
                    vec![data.clone()]
                } else {
                    frames.clone()
                },
                purpose: match mode {
                    QrMode::Display => PresentationQrPurpose::Display,
                    QrMode::Scan => PresentationQrPurpose::Capture,
                },
                label: label.clone(),
                accessibility: accessibility(a11y, label.as_deref().unwrap_or(id)),
            }),
            Component::Preview {
                name,
                initials,
                image_data,
                variants,
                selected_variant,
                visible_fields,
                a11y,
                ..
            } => self.preview(PreviewProjection {
                name,
                initials,
                image_data,
                variants,
                selected_variant,
                visible_fields,
                a11y,
            }),
            Component::Dropdown {
                id,
                label,
                selected,
                options,
                a11y,
            } => {
                let binding_id = self.binding(ValueRoute::Choice {
                    component_id: id.clone(),
                })?;
                Ok(PresentationNode::Choice {
                    binding_id,
                    label: label.clone(),
                    selected: selected.clone(),
                    options: options
                        .iter()
                        .map(|option| ChoiceOption {
                            id: option.id.clone(),
                            label: option.label.clone(),
                        })
                        .collect(),
                    enabled: true,
                    accessibility: accessibility(a11y, label),
                })
            }
            Component::ImageCircle {
                id,
                image_data,
                initials,
                brightness,
                editable,
                edit_action_id,
                a11y,
                ..
            } => {
                let activation = if *editable {
                    edit_action_id
                        .as_ref()
                        .map(|action_id| {
                            self.action(
                                "Edit",
                                accessibility(a11y, "Edit image"),
                                ActionTone::Standard,
                                UserAction::ActionPressed {
                                    action_id: action_id.clone(),
                                },
                            )
                        })
                        .transpose()?
                } else {
                    None
                };
                Ok(PresentationNode::Image {
                    id: Some(BindingId::new(id)?),
                    data: image_data.clone(),
                    fallback_text: Some(initials.clone()),
                    shape: PresentationImageShape::Circle,
                    brightness: *brightness,
                    activation,
                    accessibility: accessibility(a11y, initials),
                })
            }
            Component::Divider => Ok(PresentationNode::Divider),
            other => self.remaining_component(other),
        }
    }
}
