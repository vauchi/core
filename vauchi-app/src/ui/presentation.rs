// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{Command, Event, PaneLayout, PresentationProfile, SurfaceId, WindowClass};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PresentationCoordinatorError {
    #[error("presentation environment has not been reported")]
    EnvironmentMissing,
    #[error("surface is not visible")]
    SurfaceNotVisible,
    #[error("surface is not active")]
    SurfaceNotActive,
    #[error("event is not handled by the presentation coordinator")]
    UnsupportedEvent,
}

#[derive(Clone, Debug)]
pub struct PresentationCoordinator {
    primary_surface: SurfaceId,
    detail_surface: Option<SurfaceId>,
    active_surface: SurfaceId,
    window_class: Option<WindowClass>,
}

impl PresentationCoordinator {
    pub fn new(primary_surface: SurfaceId) -> Self {
        Self {
            active_surface: primary_surface.clone(),
            primary_surface,
            detail_surface: None,
            window_class: None,
        }
    }

    pub fn set_detail_surface(&mut self, detail_surface: Option<SurfaceId>) {
        if self.active_surface != self.primary_surface
            && detail_surface.as_ref() != Some(&self.active_surface)
        {
            self.active_surface = self.primary_surface.clone();
        }
        self.detail_surface = detail_surface;
    }

    pub fn set_primary_surface(&mut self, primary_surface: SurfaceId) {
        if self.primary_surface != primary_surface {
            self.primary_surface = primary_surface.clone();
            self.detail_surface = None;
            self.active_surface = primary_surface;
        }
    }

    #[cfg(feature = "network-rustls")]
    pub(crate) fn configure_surfaces(
        &mut self,
        primary_surface: SurfaceId,
        detail_surface: Option<SurfaceId>,
        active_surface: SurfaceId,
    ) {
        let active_is_visible =
            active_surface == primary_surface || detail_surface.as_ref() == Some(&active_surface);
        self.primary_surface = primary_surface.clone();
        self.detail_surface = detail_surface;
        self.active_surface = if active_is_visible {
            active_surface
        } else {
            primary_surface
        };
    }

    #[cfg(feature = "network-rustls")]
    pub(crate) fn current_profile_command(&self) -> Option<Command> {
        self.profile()
            .ok()
            .map(|profile| Command::SetPresentationProfile { profile })
    }

    #[cfg(feature = "network-rustls")]
    pub(crate) fn ensure_active_surface(
        &self,
        surface_id: &SurfaceId,
    ) -> Result<(), PresentationCoordinatorError> {
        if surface_id != &self.active_surface {
            return Err(PresentationCoordinatorError::SurfaceNotActive);
        }
        Ok(())
    }

    pub fn handle_event(
        &mut self,
        event: Event,
    ) -> Result<Vec<Command>, PresentationCoordinatorError> {
        match event {
            Event::PresentationEnvironmentChanged {
                available_width, ..
            } => {
                self.window_class = Some(classify_width(available_width));
            }
            Event::SurfaceActivated { surface_id } => {
                if !self.surface_is_visible(&surface_id)? {
                    return Err(PresentationCoordinatorError::SurfaceNotVisible);
                }
                self.active_surface = surface_id;
            }
            _ => return Err(PresentationCoordinatorError::UnsupportedEvent),
        }

        Ok(vec![Command::SetPresentationProfile {
            profile: self.profile()?,
        }])
    }

    fn profile(&self) -> Result<PresentationProfile, PresentationCoordinatorError> {
        let window_class = self
            .window_class
            .ok_or(PresentationCoordinatorError::EnvironmentMissing)?;
        let pane_layout = if window_class == WindowClass::Compact || self.detail_surface.is_none() {
            PaneLayout::Single
        } else {
            PaneLayout::Split
        };

        Ok(PresentationProfile {
            window_class,
            pane_layout,
            primary_surface: self.primary_surface.clone(),
            detail_surface: self.detail_surface.clone(),
            active_surface: self.active_surface.clone(),
        })
    }

    fn surface_is_visible(
        &self,
        surface_id: &SurfaceId,
    ) -> Result<bool, PresentationCoordinatorError> {
        let profile = self.profile()?;
        Ok(surface_id == &profile.active_surface
            || (profile.pane_layout == PaneLayout::Split
                && (surface_id == &profile.primary_surface
                    || profile.detail_surface.as_ref() == Some(surface_id))))
    }
}

fn classify_width(available_width: u32) -> WindowClass {
    if available_width < 600 {
        WindowClass::Compact
    } else if available_width < 840 {
        WindowClass::Medium
    } else {
        WindowClass::Expanded
    }
}
