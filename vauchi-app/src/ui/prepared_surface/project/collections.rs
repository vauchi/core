// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{
    AccessibilitySpec, ActionTone, ChoiceOption, PresentationImageShape, PresentationNode,
    PresentationPaging, PresentationRow,
};

use super::{
    PreviewProjection, Projection, ValueRoute, accessibility, group, setting_is_destructive,
};
use crate::ui::{
    ActionListItem, Item, PreparedSurfaceError, SettingsItem, SettingsItemKind, UiFieldVisibility,
    UserAction,
};

impl Projection {
    pub(super) fn list(
        &mut self,
        id: &str,
        items: &[Item],
        searchable: bool,
        total_count: usize,
        offset: usize,
        window: usize,
    ) -> Result<PresentationNode, PreparedSurfaceError> {
        let mut rows = Vec::with_capacity(items.len());
        for item in items {
            let activation = self.action(
                &item.name,
                accessibility(&item.a11y, &item.name),
                ActionTone::Standard,
                UserAction::ListItemSelected {
                    component_id: id.to_owned(),
                    item_id: item.id.clone(),
                },
            )?;
            let mut secondary_actions = Vec::with_capacity(item.actions.len());
            for action in &item.actions {
                secondary_actions.push(self.action(
                    &action.label,
                    AccessibilitySpec::label(&action.label),
                    if action.destructive {
                        ActionTone::Destructive
                    } else {
                        ActionTone::default()
                    },
                    UserAction::ListItemAction {
                        component_id: id.to_owned(),
                        item_id: item.id.clone(),
                        action_id: action.id.clone(),
                    },
                )?);
            }
            rows.push(PresentationRow {
                title: item.name.clone(),
                subtitle: item.subtitle.clone(),
                detail: item.status.clone(),
                icon_token: None,
                image_data: None,
                fallback_text: Some(item.initials.clone()),
                selected: false,
                enabled: true,
                activation: Some(activation),
                secondary_actions,
                controls: Vec::new(),
                accessibility: accessibility(&item.a11y, &item.name),
            });
        }
        Ok(PresentationNode::List {
            id: vauchi_core::BindingId::new(id)?,
            label: None,
            rows,
            searchable,
            paging: (total_count > 0).then_some(PresentationPaging {
                total_count,
                offset,
                window,
            }),
            accessibility: AccessibilitySpec::label(id),
        })
    }

    pub(super) fn settings_group(
        &mut self,
        id: &str,
        label: &str,
        items: &[SettingsItem],
    ) -> Result<PresentationNode, PreparedSurfaceError> {
        let mut rows = Vec::with_capacity(items.len());
        for item in items {
            let (detail, activation, controls) = match &item.kind {
                SettingsItemKind::Toggle { enabled } => {
                    let binding_id = self.binding(ValueRoute::SettingsToggle {
                        component_id: id.to_owned(),
                        item_id: item.id.clone(),
                    })?;
                    (
                        None,
                        None,
                        vec![PresentationNode::Toggle {
                            binding_id,
                            label: item.label.clone(),
                            value: *enabled,
                            enabled: true,
                            accessibility: accessibility(&item.a11y, &item.label),
                        }],
                    )
                }
                SettingsItemKind::Value { value } => (
                    Some(value.clone()),
                    Some(self.settings_action(item)?),
                    Vec::new(),
                ),
                SettingsItemKind::Link { detail } => (
                    detail.clone(),
                    Some(self.settings_action(item)?),
                    Vec::new(),
                ),
                SettingsItemKind::Destructive { label } => (
                    Some(label.clone()),
                    Some(self.settings_action(item)?),
                    Vec::new(),
                ),
            };
            rows.push(PresentationRow {
                title: item.label.clone(),
                subtitle: None,
                detail,
                icon_token: None,
                image_data: None,
                fallback_text: None,
                selected: false,
                enabled: true,
                activation,
                secondary_actions: Vec::new(),
                controls,
                accessibility: accessibility(&item.a11y, &item.label),
            });
        }
        Ok(PresentationNode::List {
            id: vauchi_core::BindingId::new(id)?,
            label: Some(label.to_owned()),
            rows,
            searchable: false,
            paging: None,
            accessibility: AccessibilitySpec::label(label),
        })
    }

    fn settings_action(
        &mut self,
        item: &SettingsItem,
    ) -> Result<vauchi_core::ActionSpec, PreparedSurfaceError> {
        self.action(
            &item.label,
            accessibility(&item.a11y, &item.label),
            if setting_is_destructive(&item.kind) {
                ActionTone::Destructive
            } else {
                ActionTone::Standard
            },
            UserAction::ActionPressed {
                action_id: item.id.clone(),
            },
        )
    }

    pub(super) fn action_list(
        &mut self,
        id: &str,
        label: Option<String>,
        items: &[ActionListItem],
    ) -> Result<PresentationNode, PreparedSurfaceError> {
        let mut rows = Vec::with_capacity(items.len());
        for item in items {
            let activation = self.action(
                &item.label,
                accessibility(&item.a11y, &item.label),
                ActionTone::Standard,
                UserAction::ListItemSelected {
                    component_id: id.to_owned(),
                    item_id: item.id.clone(),
                },
            )?;
            rows.push(PresentationRow {
                title: item.label.clone(),
                subtitle: item.detail.clone(),
                detail: None,
                icon_token: item.icon.clone(),
                image_data: None,
                fallback_text: None,
                selected: false,
                enabled: true,
                activation: Some(activation),
                secondary_actions: Vec::new(),
                controls: Vec::new(),
                accessibility: accessibility(&item.a11y, &item.label),
            });
        }
        Ok(PresentationNode::List {
            id: vauchi_core::BindingId::new(id)?,
            label: label.clone(),
            rows,
            searchable: false,
            paging: None,
            accessibility: AccessibilitySpec::label(label.as_deref().unwrap_or(id)),
        })
    }

    pub(super) fn preview(
        &mut self,
        preview: PreviewProjection<'_>,
    ) -> Result<PresentationNode, PreparedSurfaceError> {
        let PreviewProjection {
            name,
            initials,
            image_data,
            variants,
            selected_variant,
            visible_fields,
            a11y,
        } = preview;
        let mut children = vec![PresentationNode::Image {
            id: None,
            data: image_data.clone(),
            fallback_text: Some(initials.to_owned()),
            shape: PresentationImageShape::Circle,
            brightness: 0.0,
            activation: None,
            accessibility: accessibility(a11y, name),
        }];
        if !variants.is_empty() {
            let binding_id = self.binding(ValueRoute::Variant)?;
            children.push(PresentationNode::Choice {
                binding_id,
                label: name.to_owned(),
                selected: selected_variant.clone(),
                options: variants
                    .iter()
                    .map(|variant| ChoiceOption {
                        id: variant.variant_id.clone(),
                        label: variant.display_name.clone(),
                    })
                    .collect(),
                enabled: true,
                accessibility: AccessibilitySpec::label(name),
            });
        }
        let field_rows = visible_fields
            .iter()
            .map(|field| PresentationRow {
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
                controls: Vec::new(),
                accessibility: accessibility(&field.a11y, &field.label),
            })
            .collect();
        children.push(PresentationNode::List {
            id: self.node_id()?,
            label: None,
            rows: field_rows,
            searchable: false,
            paging: None,
            accessibility: accessibility(a11y, name),
        });
        Ok(group(
            None,
            Some(name.to_owned()),
            children,
            accessibility(a11y, name),
        ))
    }
}
