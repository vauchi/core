// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;

use vauchi_core::{
    BindingId, Command, Event, InputValue, InteractionId, PresentationIdError, PresentationTokens,
    SurfaceId, SurfaceLayout, SurfaceSpec,
};

use super::{ScreenLayout, ScreenModel, UserAction};

mod project;

use project::{Projection, ValueRoute};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PreparedSurfaceError {
    #[error("invalid opaque presentation identifier: {0}")]
    InvalidIdentifier(#[from] PresentationIdError),
    #[error("component does not yet have a generic presentation projection")]
    UnsupportedComponent,
    #[error("event targets a different surface")]
    SurfaceMismatch,
    #[error("event binding is not active on this surface")]
    UnknownBinding,
    #[error("event value has the wrong type for this binding")]
    ValueTypeMismatch,
    #[error("event is not handled by the prepared surface")]
    UnsupportedEvent,
}

#[derive(Clone, Debug)]
pub struct PreparedSurface {
    surface: SurfaceSpec,
    value_routes: HashMap<BindingId, ValueRoute>,
    interaction_routes: HashMap<InteractionId, UserAction>,
}

impl PreparedSurface {
    pub fn from_screen(
        surface_id: SurfaceId,
        revision: u64,
        screen: &ScreenModel,
    ) -> Result<Self, PreparedSurfaceError> {
        let mut projection = Projection::new(revision);
        let mut nodes = Vec::with_capacity(screen.components.len());
        for component in &screen.components {
            nodes.push(projection.component(component)?);
        }
        if let Some(progress) = &screen.progress {
            let value = (progress.total_steps > 0)
                .then(|| f64::from(progress.current_step) / f64::from(progress.total_steps));
            nodes.push(vauchi_core::PresentationNode::Progress {
                label: progress.label.clone(),
                value,
                accessibility: vauchi_core::AccessibilitySpec::label(
                    progress.label.as_deref().unwrap_or("Progress"),
                ),
            });
        }

        let tokens = &screen.tokens;
        Ok(Self {
            surface: SurfaceSpec {
                surface_id,
                revision,
                title: screen.title.clone(),
                subtitle: screen.subtitle.clone(),
                accessibility_label: screen.title.clone(),
                layout: match screen.layout {
                    ScreenLayout::Scroll => SurfaceLayout::Scroll,
                    ScreenLayout::Fixed => SurfaceLayout::Fixed,
                    ScreenLayout::Pinned => SurfaceLayout::Pinned,
                },
                tokens: PresentationTokens {
                    spacing_small: tokens.spacing.sm,
                    spacing_medium: tokens.spacing.md,
                    spacing_large: tokens.spacing.lg,
                    corner_radius: tokens.border_radius.md_lg,
                    minimum_target_size: tokens.touch_target.minimum,
                },
                nodes,
            },
            value_routes: projection.value_routes,
            interaction_routes: projection.interaction_routes,
        })
    }

    pub fn command(&self) -> Command {
        Command::ReplaceSurface {
            surface: self.surface.clone(),
        }
    }

    /// Whether both projections give every opaque id the same meaning.
    ///
    /// Ids are minted from a positional counter, so an id set alone cannot
    /// tell "same surface, new content" from "different component in the same
    /// slot". Comparing the routes an id resolves to distinguishes them, which
    /// is what decides whether the surface revision has to advance.
    ///
    /// Gated with the sole caller in `ui::app_engine`, which the
    /// no-default-features build compiles out.
    #[cfg(feature = "network-rustls")]
    pub(crate) fn routes_match(&self, other: &Self) -> bool {
        self.value_routes == other.value_routes
            && self.interaction_routes == other.interaction_routes
    }

    pub fn reduce(&self, event: Event) -> Result<UserAction, PreparedSurfaceError> {
        match event {
            Event::ValueChanged {
                surface_id,
                binding_id,
                value,
            } => {
                self.ensure_surface(&surface_id)?;
                reduce_value(
                    self.value_routes
                        .get(&binding_id)
                        .ok_or(PreparedSurfaceError::UnknownBinding)?,
                    value,
                )
            }
            // Both resolve through the same binding map as `ValueChanged`,
            // so an id the surface does not own fails closed rather than
            // reaching a screen. Only text bindings can be submitted or
            // blurred; anything else is an unknown binding.
            Event::InputSubmitted {
                surface_id,
                binding_id,
            } => {
                self.ensure_surface(&surface_id)?;
                match self
                    .value_routes
                    .get(&binding_id)
                    .ok_or(PreparedSurfaceError::UnknownBinding)?
                {
                    ValueRoute::Text { component_id } => Ok(UserAction::TextSubmitted {
                        component_id: component_id.clone(),
                    }),
                    _ => Err(PreparedSurfaceError::UnknownBinding),
                }
            }
            Event::InputFocusEnded {
                surface_id,
                binding_id,
            } => {
                self.ensure_surface(&surface_id)?;
                match self
                    .value_routes
                    .get(&binding_id)
                    .ok_or(PreparedSurfaceError::UnknownBinding)?
                {
                    ValueRoute::Text { component_id } => Ok(UserAction::TextFocusEnded {
                        component_id: component_id.clone(),
                    }),
                    _ => Err(PreparedSurfaceError::UnknownBinding),
                }
            }
            Event::ActionActivated {
                surface_id,
                interaction_id,
            } => {
                self.ensure_surface(&surface_id)?;
                self.interaction_routes
                    .get(&interaction_id)
                    .cloned()
                    .ok_or(PreparedSurfaceError::UnknownBinding)
            }
            _ => Err(PreparedSurfaceError::UnsupportedEvent),
        }
    }

    fn ensure_surface(&self, surface_id: &SurfaceId) -> Result<(), PreparedSurfaceError> {
        if surface_id == &self.surface.surface_id {
            Ok(())
        } else {
            Err(PreparedSurfaceError::SurfaceMismatch)
        }
    }
}

fn reduce_value(route: &ValueRoute, value: InputValue) -> Result<UserAction, PreparedSurfaceError> {
    match route {
        ValueRoute::Text { component_id } => {
            let InputValue::Text(value) = value else {
                return Err(PreparedSurfaceError::ValueTypeMismatch);
            };
            Ok(UserAction::TextChanged {
                component_id: component_id.clone(),
                value,
            })
        }
        ValueRoute::ToggleItem {
            component_id,
            item_id,
        } => {
            let InputValue::Boolean(_) = value else {
                return Err(PreparedSurfaceError::ValueTypeMismatch);
            };
            Ok(UserAction::ItemToggled {
                component_id: component_id.clone(),
                item_id: item_id.clone(),
            })
        }
        ValueRoute::SettingsToggle {
            component_id,
            item_id,
        } => {
            let InputValue::Boolean(_) = value else {
                return Err(PreparedSurfaceError::ValueTypeMismatch);
            };
            Ok(UserAction::SettingsToggled {
                component_id: component_id.clone(),
                item_id: item_id.clone(),
            })
        }
        ValueRoute::Choice { component_id } => {
            let InputValue::Choice(Some(item_id)) = value else {
                return Err(PreparedSurfaceError::ValueTypeMismatch);
            };
            Ok(UserAction::ListItemSelected {
                component_id: component_id.clone(),
                item_id,
            })
        }
        ValueRoute::Variant => {
            let InputValue::Choice(variant_id) = value else {
                return Err(PreparedSurfaceError::ValueTypeMismatch);
            };
            Ok(UserAction::VariantSelected { variant_id })
        }
        ValueRoute::Slider { component_id } => {
            let InputValue::Number(value) = value else {
                return Err(PreparedSurfaceError::ValueTypeMismatch);
            };
            Ok(UserAction::SliderChanged {
                component_id: component_id.clone(),
                value_milli: (value * 1000.0).round() as i32,
            })
        }
        ValueRoute::FieldVisibility { field_id, group_id } => {
            let InputValue::Boolean(visible) = value else {
                return Err(PreparedSurfaceError::ValueTypeMismatch);
            };
            Ok(UserAction::FieldVisibilityChanged {
                field_id: field_id.clone(),
                group_id: group_id.clone(),
                visible,
            })
        }
    }
}
