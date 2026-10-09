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

/// Surfaces recently left behind, remembered so a shell event still in
/// flight for one of them reads as a race, not a defect (vauchi/private#438).
const LEFT_BEHIND_MEMORY: usize = 4;

#[derive(Clone, Debug)]
pub struct PresentationCoordinator {
    primary_surface: SurfaceId,
    detail_surface: Option<SurfaceId>,
    active_surface: SurfaceId,
    window_class: Option<WindowClass>,
    available_height: Option<u32>,
    left_behind: Vec<SurfaceId>,
}

impl PresentationCoordinator {
    pub fn new(primary_surface: SurfaceId) -> Self {
        Self {
            active_surface: primary_surface.clone(),
            primary_surface,
            detail_surface: None,
            window_class: None,
            available_height: None,
            left_behind: Vec::new(),
        }
    }

    fn activate(&mut self, surface: SurfaceId) {
        if surface == self.active_surface {
            return;
        }
        self.left_behind.retain(|left| left != &surface);
        let previous = std::mem::replace(&mut self.active_surface, surface);
        self.left_behind.push(previous);
        if self.left_behind.len() > LEFT_BEHIND_MEMORY {
            self.left_behind.remove(0);
        }
    }

    /// Whether `surface` was shown a moment ago and no longer is: an event
    /// for it is a shell still catching up, not a malformed one. A pane
    /// still on screen but inactive is not left behind; it must be
    /// activated before it takes input.
    #[cfg(feature = "network-rustls")]
    pub(crate) fn was_left_behind(&self, surface: &SurfaceId) -> bool {
        surface != &self.active_surface
            && surface != &self.primary_surface
            && self.detail_surface.as_ref() != Some(surface)
            && self.left_behind.contains(surface)
    }

    pub fn set_detail_surface(&mut self, detail_surface: Option<SurfaceId>) {
        if self.active_surface != self.primary_surface
            && detail_surface.as_ref() != Some(&self.active_surface)
        {
            self.activate(self.primary_surface.clone());
        }
        self.detail_surface = detail_surface;
    }

    pub fn set_primary_surface(&mut self, primary_surface: SurfaceId) {
        if self.primary_surface != primary_surface {
            self.primary_surface = primary_surface.clone();
            self.detail_surface = None;
            self.activate(primary_surface);
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
        self.activate(if active_is_visible {
            active_surface
        } else {
            primary_surface
        });
    }

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
                available_width,
                available_height,
                ..
            } => {
                self.window_class = Some(classify_width(self.window_class, available_width));
                self.available_height = Some(available_height);
            }
            Event::SurfaceActivated { surface_id } => {
                if !self.surface_is_visible(&surface_id)? {
                    return Err(PresentationCoordinatorError::SurfaceNotVisible);
                }
                self.activate(surface_id);
            }
            _ => return Err(PresentationCoordinatorError::UnsupportedEvent),
        }

        Ok(vec![Command::SetPresentationProfile {
            profile: self.profile()?,
        }])
    }

    /// Whether the window is too short for a fixed screen's full layout.
    /// Unknown until the shell reports its environment, and then not short.
    #[cfg(feature = "network-rustls")]
    pub(crate) fn is_short(&self) -> bool {
        self.available_height
            .is_some_and(|height| height < SHORT_WINDOW_HEIGHT)
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

/// Below this height, in logical units, an exchange screen's code, its
/// heading, the mode row and a usable camera do not all fit. An iPhone SE
/// (667 pt screen) is below it: its Glance camera was 0 pt tall (#513).
#[cfg(feature = "network-rustls")]
const SHORT_WINDOW_HEIGHT: u32 = 720;

/// Collapse lags expansion by this many logical units, so a window resting
/// near a threshold (scrollbar toggling, edge drag) does not flip layouts
/// (ADR-066 class-boundary hysteresis).
const COLLAPSE_BAND: u32 = 32;

fn classify_width(previous: Option<WindowClass>, available_width: u32) -> WindowClass {
    let plain = threshold_class(available_width);
    let Some(previous) = previous else {
        return plain;
    };
    let damped = threshold_class(available_width.saturating_add(COLLAPSE_BAND));
    let held = if rank(previous) < rank(damped) {
        previous
    } else {
        damped
    };
    if rank(held) > rank(plain) {
        held
    } else {
        plain
    }
}

fn threshold_class(available_width: u32) -> WindowClass {
    if available_width < 600 {
        WindowClass::Compact
    } else if available_width < 840 {
        WindowClass::Medium
    } else {
        WindowClass::Expanded
    }
}

fn rank(class: WindowClass) -> u8 {
    match class {
        WindowClass::Compact => 0,
        WindowClass::Medium => 1,
        _ => 2,
    }
}
