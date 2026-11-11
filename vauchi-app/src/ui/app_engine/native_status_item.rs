// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{AppEngine, AppPresentationError};
use crate::ui::ContextualSurfaceError;
use vauchi_core::{Command, StatusItemSpec, SurfaceId};

impl AppEngine {
    fn status_item(&self, item: Option<StatusItemSpec>) -> Result<Command, AppPresentationError> {
        Ok(Command::SetStatusItem {
            surface_id: SurfaceId::new(self.screen.screen_id())
                .map_err(ContextualSurfaceError::from)?,
            revision: self.surface_revision,
            item,
        })
    }

    pub(super) fn status_item_command(&self) -> Result<Option<Command>, AppPresentationError> {
        if !self.status_item_available {
            return Ok(None);
        }
        self.status_item(Some(StatusItemSpec {
            tooltip: self.t("app.name"),
            show_window_label: self.t("platform.tray_show"),
            items: Vec::new(),
        }))
        .map(Some)
    }

    pub(super) fn set_status_item_available(
        &mut self,
        available: bool,
    ) -> Result<Vec<Command>, AppPresentationError> {
        self.status_item_available = available;
        if available {
            self.initial_commands()
        } else {
            Ok(vec![self.status_item(None)?])
        }
    }
}
