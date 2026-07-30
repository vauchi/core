// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{AccessibilitySpec, ActionTone, PresentationNode, PresentationTone};

use super::{Projection, ValueRoute};
use crate::ui::{Field, PreparedSurfaceError, UserAction};

impl Projection {
    pub(super) fn visibility_toggle(
        &mut self,
        field: &Field,
        group_id: Option<String>,
        visible: bool,
    ) -> Result<PresentationNode, PreparedSurfaceError> {
        let label = group_id.as_deref().unwrap_or(&field.label).to_owned();
        Ok(PresentationNode::Toggle {
            binding_id: self.binding(ValueRoute::FieldVisibility {
                field_id: field.id.clone(),
                group_id,
            })?,
            label: label.clone(),
            value: visible,
            enabled: true,
            accessibility: AccessibilitySpec::label(label),
        })
    }

    pub(super) fn action_node(
        &mut self,
        label: &str,
        action_id: &str,
    ) -> Result<PresentationNode, PreparedSurfaceError> {
        Ok(PresentationNode::Status {
            id: None,
            title: label.to_owned(),
            detail: None,
            icon_token: None,
            badge: None,
            tone: PresentationTone::Neutral,
            activation: Some(self.action(
                label,
                AccessibilitySpec::label(label),
                ActionTone::Standard,
                UserAction::ActionPressed {
                    action_id: action_id.to_owned(),
                },
            )?),
            accessibility: AccessibilitySpec::label(label),
        })
    }
}
