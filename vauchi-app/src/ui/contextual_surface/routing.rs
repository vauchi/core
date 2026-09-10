// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{Command, ContextBar, Event, SurfaceId};

use super::{ContextualSurface, ContextualSurfaceError, ContextualSurfaceRoute};
use crate::ui::UserAction;

impl ContextualSurface {
    pub fn context_bar(&self) -> &ContextBar {
        &self.bar
    }

    /// The persistent navigation command for this surface — a shell that
    /// renders a bottom bar or sidebar needs it whenever the context bar is
    /// (re)installed, not only when `initial_commands` runs the whole
    /// batch. `AppEngine::surface_commands` calls this directly because it
    /// sources `SetContextBar` from `ContextualActionCoordinator` (which
    /// owns Undo substitution), not from `initial_commands` below.
    pub fn navigation_command(&self) -> Command {
        Command::SetNavigation {
            surface_id: self.surface_id.clone(),
            revision: self.revision,
            navigation: self.navigation_spec.clone(),
        }
    }

    pub fn initial_commands(&self) -> Vec<Command> {
        vec![
            Command::SetContextBar {
                surface_id: self.surface_id.clone(),
                revision: self.revision,
                bar: Box::new(self.bar.clone()),
            },
            self.navigation_command(),
        ]
    }

    pub fn handle_event(
        &self,
        event: Event,
    ) -> Result<ContextualSurfaceRoute, ContextualSurfaceError> {
        match event {
            Event::BackRequested { surface_id } => {
                self.ensure_surface(&surface_id)?;
                Ok(ContextualSurfaceRoute::UserAction(UserAction::NavigateBack))
            }
            Event::ActionActivated {
                surface_id,
                interaction_id,
            } => {
                self.ensure_surface(&surface_id)?;
                if self.back_interaction_id.as_ref() == Some(&interaction_id) {
                    Ok(ContextualSurfaceRoute::UserAction(UserAction::NavigateBack))
                } else if self.navigation_interaction_id.as_ref() == Some(&interaction_id) {
                    Ok(ContextualSurfaceRoute::Commands(vec![
                        Command::PresentOverlay {
                            surface_id: self.surface_id.clone(),
                            revision: self.revision,
                            overlay: self.navigation_overlay.clone(),
                        },
                    ]))
                } else if self.secondary_interaction_id.as_ref() == Some(&interaction_id) {
                    Ok(ContextualSurfaceRoute::Commands(vec![
                        Command::PresentOverlay {
                            surface_id: self.surface_id.clone(),
                            revision: self.revision,
                            overlay: self.secondary_overlay.clone(),
                        },
                    ]))
                } else {
                    self.routes.get(&interaction_id).cloned().map_or_else(
                        || {
                            Err(ContextualSurfaceError::UnknownInteraction(
                                interaction_id.as_str().to_owned(),
                            ))
                        },
                        |action| Ok(ContextualSurfaceRoute::UserAction(action)),
                    )
                }
            }
            Event::OverlayDismissed { surface_id, .. } => {
                self.ensure_surface(&surface_id)?;
                Ok(ContextualSurfaceRoute::Commands(Vec::new()))
            }
            _ => Err(ContextualSurfaceError::UnsupportedEvent),
        }
    }

    fn ensure_surface(&self, surface_id: &SurfaceId) -> Result<(), ContextualSurfaceError> {
        if surface_id == &self.surface_id {
            Ok(())
        } else {
            Err(ContextualSurfaceError::SurfaceMismatch)
        }
    }
}
