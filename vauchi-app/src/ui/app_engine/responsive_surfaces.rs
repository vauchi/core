// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core-owned parent/detail surface composition.

use vauchi_core::SurfaceId;

use super::{AppEngine, AppPresentationError, AppScreen};
use crate::ui::{PreparedSurface, ScreenModel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CompanionRole {
    Primary,
    Detail,
}

pub(super) struct ResponsiveCompanion {
    pub screen: AppScreen,
    pub surface_id: SurfaceId,
    pub model: ScreenModel,
    pub prepared: PreparedSurface,
    pub role: CompanionRole,
}

impl AppEngine {
    pub(super) fn responsive_companion_surface(
        &self,
    ) -> Result<Option<ResponsiveCompanion>, AppPresentationError> {
        let companion = if let Some(parent_id) = self.screen.parent_screen_id() {
            let Some(parent_screen) = AppScreen::from_screen_id(parent_id) else {
                return Ok(None);
            };
            Some((parent_screen, CompanionRole::Primary))
        } else {
            self.retained_detail_screen
                .as_ref()
                .filter(|detail| detail.parent_screen_id() == Some(self.screen.screen_id()))
                .cloned()
                .map(|detail| (detail, CompanionRole::Detail))
        };
        let Some((companion_screen, role)) = companion else {
            return Ok(None);
        };
        let surface_id = SurfaceId::new(companion_screen.screen_id())
            .map_err(crate::ui::ContextualSurfaceError::from)?;
        let model = self
            .engine_cache
            .get(&companion_screen)
            .map(|engine| engine.current_screen())
            .unwrap_or_else(|| {
                Self::create_engine(
                    &self.vauchi,
                    &companion_screen,
                    self.preview_as_contact.as_deref(),
                    &self.device_capabilities,
                    &self.transport_readiness,
                    &self.render_context,
                    &self.pending_exchange_groups,
                    self.glance_display_qr.as_deref(),
                )
                .current_screen()
            });
        let prepared =
            PreparedSurface::from_screen(surface_id.clone(), self.surface_revision, &model)?;
        Ok(Some(ResponsiveCompanion {
            screen: companion_screen,
            surface_id,
            model,
            prepared,
            role,
        }))
    }
}
