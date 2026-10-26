// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{AppEngine, AppPresentationError};
use crate::ui::{ContextualSurfaceError, UserAction};
use vauchi_core::{
    ActionSpec, ActionTone, ApplicationMenuRole, ApplicationMenuSpec, Command, Event,
    InteractionId, StandardShortcut, SurfaceId,
};

impl AppEngine {
    pub(super) fn native_import_enabled(&self) -> bool {
        self.native_menu_available
            && !self.session_locked
            && self.vauchi.has_identity()
            && !self.pending_contact_import
    }

    fn native_import_action(&self) -> Result<ActionSpec, AppPresentationError> {
        let label = self.t("menu.import_contacts");
        Ok(ActionSpec {
            interaction_id: InteractionId::new(format!(
                "presentation.application.{}.0",
                self.surface_revision
            ))
            .map_err(ContextualSurfaceError::from)?,
            accessibility_label: label.clone(),
            label,
            icon_token: None,
            enabled: self.native_import_enabled(),
            tone: ActionTone::Standard,
            shortcut: Some(StandardShortcut::Import),
        })
    }

    pub(super) fn application_menu_command(&self) -> Result<Option<Command>, AppPresentationError> {
        if !self.native_menu_available {
            return Ok(None);
        }
        Ok(Some(Command::SetApplicationMenu {
            surface_id: SurfaceId::new(self.screen.screen_id())
                .map_err(ContextualSurfaceError::from)?,
            revision: self.surface_revision,
            menus: vec![ApplicationMenuSpec {
                role: ApplicationMenuRole::File,
                label: self.t("menu.file"),
                items: vec![self.native_import_action()?],
            }],
        }))
    }

    pub(super) fn set_application_menu_available(
        &mut self,
        available: bool,
    ) -> Result<Vec<Command>, AppPresentationError> {
        self.native_menu_available = available;
        if available {
            self.initial_commands()
        } else {
            Ok(vec![Command::SetApplicationMenu {
                surface_id: SurfaceId::new(self.screen.screen_id())
                    .map_err(ContextualSurfaceError::from)?,
                revision: self.surface_revision,
                menus: Vec::new(),
            }])
        }
    }

    pub(super) fn native_menu_action(
        &self,
        event: &Event,
    ) -> Result<Option<UserAction>, AppPresentationError> {
        let Event::ActionActivated {
            surface_id,
            interaction_id,
        } = event
        else {
            return Ok(None);
        };
        if !self.native_menu_available || surface_id.as_str() != self.screen.screen_id() {
            return Ok(None);
        }
        let action = self.native_import_action()?;
        if *interaction_id != action.interaction_id {
            return Ok(None);
        }
        if !action.enabled {
            return Err(
                ContextualSurfaceError::UnknownInteraction(interaction_id.as_str().into()).into(),
            );
        }
        Ok(Some(UserAction::ActionPressed {
            action_id: "import_contacts_from_menu".into(),
        }))
    }
}
