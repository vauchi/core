// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{
    AccessibilitySpec, ActionTone, ChoiceOption, PresentationImageShape, PresentationListStyle,
    PresentationNode, PresentationPaging, PresentationRow,
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
            accessibility: AccessibilitySpec::label(""),
            style: PresentationListStyle::Rows,
        })
    }

    /// A toggle list whose items explain themselves (a subtitle, such as
    /// why a contact sees an entry) projects to rows with a toggle control,
    /// the shape settings rows already use, so every shell draws the
    /// explanation — a bare `Toggle` node carries only its label (#428).
    pub(super) fn toggle_rows(
        &mut self,
        id: &str,
        label: &str,
        items: &[crate::ui::ToggleItem],
        a11y: &Option<crate::ui::A11y>,
    ) -> Result<PresentationNode, PreparedSurfaceError> {
        let mut rows = Vec::with_capacity(items.len());
        for item in items {
            let binding_id = self.binding(ValueRoute::ToggleItem {
                component_id: id.to_owned(),
                item_id: item.id.clone(),
            })?;
            let mut spoken = accessibility(&item.a11y, &item.label);
            if spoken.description.is_none() {
                spoken.description = item.subtitle.clone();
            }
            rows.push(PresentationRow {
                title: item.label.clone(),
                subtitle: item.subtitle.clone(),
                detail: None,
                icon_token: None,
                image_data: None,
                fallback_text: None,
                selected: false,
                enabled: true,
                activation: None,
                secondary_actions: Vec::new(),
                // The row title already names the entry; the control's
                // visible label stays empty, as for settings toggles.
                controls: vec![PresentationNode::Toggle {
                    binding_id,
                    label: String::new(),
                    value: item.selected,
                    enabled: true,
                    accessibility: spoken.clone(),
                }],
                accessibility: spoken,
            });
        }
        Ok(PresentationNode::List {
            id: vauchi_core::BindingId::new(id)?,
            label: (!label.is_empty()).then(|| label.to_owned()),
            rows,
            style: PresentationListStyle::Rows,
            searchable: false,
            paging: None,
            accessibility: accessibility(a11y, label),
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
                        // The row title already names the setting. Repeating
                        // it here made Android draw the name twice and iOS
                        // draw a control it could not explain, so the visible
                        // label stays empty and only assistive tech gets the
                        // name.
                        vec![PresentationNode::Toggle {
                            binding_id,
                            label: String::new(),
                            value: *enabled,
                            enabled: true,
                            accessibility: accessibility(&item.a11y, &item.label),
                        }],
                    )
                }
                SettingsItemKind::Value { value } => (
                    Some(value.clone()),
                    Some(self.settings_action(id, item)?),
                    Vec::new(),
                ),
                SettingsItemKind::Link { detail } => (
                    detail.clone(),
                    Some(self.settings_action(id, item)?),
                    Vec::new(),
                ),
                SettingsItemKind::Destructive { label } => (
                    (!label.is_empty()).then(|| label.clone()),
                    Some(self.settings_action(id, item)?),
                    Vec::new(),
                ),
            };
            rows.push(PresentationRow {
                title: item.label.clone(),
                subtitle: item.subtitle.clone(),
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
            label: (!label.is_empty()).then(|| label.to_owned()),
            rows,
            searchable: false,
            paging: None,
            accessibility: AccessibilitySpec::label(label),
            style: PresentationListStyle::Rows,
        })
    }

    /// Settings rows route through `ListItemSelected` — the same contract
    /// `list()`/`action_list()` use — because `intercept_settings_action`
    /// and the settings/consent engines match on `component_id` + `item_id`
    /// (GTK-3/QT-3: `ActionPressed` routed nowhere, leaving every row inert).
    fn settings_action(
        &mut self,
        component_id: &str,
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
            UserAction::ListItemSelected {
                component_id: component_id.to_owned(),
                item_id: item.id.clone(),
            },
        )
    }

    pub(super) fn action_list(
        &mut self,
        id: &str,
        label: Option<String>,
        items: &[ActionListItem],
        style: PresentationListStyle,
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
            accessibility: AccessibilitySpec::label(label.as_deref().unwrap_or("")),
            style,
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
            size: None,
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
            style: PresentationListStyle::Rows,
        });
        Ok(group(
            None,
            Some(name.to_owned()),
            children,
            accessibility(a11y, name),
        ))
    }
}
