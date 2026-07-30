// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;

use vauchi_core::{
    ActionSpec, ActionTone, Command, ContextBar, InteractionId, OverlayKind, OverlaySpec,
    PresentationIdError, StandardShortcut, SurfaceId,
};

use super::{ActionStyle, ScreenAction, ScreenModel, TabInfo, UserAction};

mod routing;

const BACK_INTERACTION_ID: &str = "presentation.back";
const NAVIGATION_INTERACTION_ID: &str = "presentation.navigation";
const SECONDARY_INTERACTION_ID: &str = "presentation.secondary";
const NAVIGATION_ITEM_PREFIX: &str = "presentation.navigation.";
const LEGACY_BACK_ACTION_ID: &str = "go_back";

#[derive(Clone, Debug, PartialEq)]
pub enum ContextualSurfaceRoute {
    Commands(Vec<Command>),
    UserAction(UserAction),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ContextualSurfaceError {
    #[error("invalid contextual interaction id: {0}")]
    InvalidInteractionId(#[from] PresentationIdError),
    #[error("screen action uses reserved contextual interaction id: {0}")]
    ReservedInteractionId(String),
    #[error("presentation event targets another surface")]
    SurfaceMismatch,
    #[error("unknown contextual interaction id: {0}")]
    UnknownInteraction(String),
    #[error("event is not handled by the contextual surface")]
    UnsupportedEvent,
}

/// Core-owned contextual controls for one rendered surface.
///
/// This is the retirement seam for `ScreenModel.contextual_actions`: Core partitions the
/// legacy engine output into the four contextual roles once, while shells only
/// render commands and return opaque interaction identifiers.
pub struct ContextualSurface {
    surface_id: SurfaceId,
    revision: u64,
    bar: ContextBar,
    back_interaction_id: Option<InteractionId>,
    navigation_interaction_id: Option<InteractionId>,
    secondary_interaction_id: Option<InteractionId>,
    navigation_overlay: OverlaySpec,
    secondary_overlay: OverlaySpec,
    routes: HashMap<InteractionId, UserAction>,
}

impl ContextualSurface {
    pub fn compose(
        surface_id: SurfaceId,
        screen: &ScreenModel,
        navigation: &[TabInfo],
        navigation_label: &str,
        secondary_label: &str,
    ) -> Result<Self, ContextualSurfaceError> {
        Self::compose_inner(
            surface_id,
            None,
            screen,
            navigation,
            navigation_label,
            secondary_label,
        )
    }

    pub fn compose_revisioned(
        surface_id: SurfaceId,
        revision: u64,
        screen: &ScreenModel,
        navigation: &[TabInfo],
        navigation_label: &str,
        secondary_label: &str,
    ) -> Result<Self, ContextualSurfaceError> {
        Self::compose_inner(
            surface_id,
            Some(revision),
            screen,
            navigation,
            navigation_label,
            secondary_label,
        )
    }

    fn compose_inner(
        surface_id: SurfaceId,
        revision: Option<u64>,
        screen: &ScreenModel,
        navigation: &[TabInfo],
        navigation_label: &str,
        secondary_label: &str,
    ) -> Result<Self, ContextualSurfaceError> {
        let mut routes = HashMap::new();
        let back_action = screen
            .nav_actions
            .iter()
            .find(|action| action.id == LEGACY_BACK_ACTION_ID);
        let primary_index = screen
            .contextual_actions
            .iter()
            .position(|action| matches!(action.style, ActionStyle::Primary));

        let back = back_action
            .map(|action| {
                action_spec(
                    scoped_interaction(revision, BACK_INTERACTION_ID)?,
                    action,
                    ActionTone::Standard,
                    Some(StandardShortcut::Back),
                )
            })
            .transpose()?;
        let navigation_action = if navigation.is_empty() {
            None
        } else {
            Some(launcher_action(
                scoped_interaction(revision, NAVIGATION_INTERACTION_ID)?,
                navigation_label,
            ))
        };
        let primary = primary_index
            .map(|index| {
                action_spec(
                    scoped_interaction(revision, &screen.contextual_actions[index].id)?,
                    &screen.contextual_actions[index],
                    ActionTone::Standard,
                    Some(StandardShortcut::ActivatePrimary),
                )
            })
            .transpose()?;

        let mut secondary_items = Vec::new();
        for (index, action) in screen.contextual_actions.iter().enumerate() {
            validate_not_reserved(&action.id)?;
            let interaction_id = scoped_interaction(revision, &action.id)?;
            routes.insert(
                interaction_id.clone(),
                UserAction::ActionPressed {
                    action_id: action.id.clone(),
                },
            );
            if Some(index) != primary_index {
                secondary_items.push(action_spec(interaction_id, action, tone_for(action), None)?);
            }
        }
        for action in screen
            .nav_actions
            .iter()
            .filter(|action| action.id != LEGACY_BACK_ACTION_ID)
        {
            validate_not_reserved(&action.id)?;
            let interaction_id = scoped_interaction(revision, &action.id)?;
            routes.insert(
                interaction_id.clone(),
                UserAction::ActionPressed {
                    action_id: action.id.clone(),
                },
            );
            secondary_items.push(action_spec(interaction_id, action, tone_for(action), None)?);
        }

        let mut navigation_items = Vec::with_capacity(navigation.len());
        for item in navigation {
            let interaction_id = scoped_interaction(
                revision,
                &format!("{NAVIGATION_ITEM_PREFIX}{}", item.action_id),
            )?;
            routes.insert(
                interaction_id.clone(),
                UserAction::NavigateToTab {
                    action_id: item.action_id.clone(),
                },
            );
            navigation_items.push(ActionSpec {
                interaction_id,
                label: item.label.clone(),
                accessibility_label: item.label.clone(),
                icon_token: Some(item.icon.clone()),
                enabled: true,
                tone: ActionTone::Standard,
                shortcut: None,
            });
        }

        let secondary = if secondary_items.is_empty() {
            None
        } else {
            Some(launcher_action(
                scoped_interaction(revision, SECONDARY_INTERACTION_ID)?,
                secondary_label,
            ))
        };
        let back_interaction_id = back.as_ref().map(|action| action.interaction_id.clone());
        let navigation_interaction_id = navigation_action
            .as_ref()
            .map(|action| action.interaction_id.clone());
        let secondary_interaction_id = secondary
            .as_ref()
            .map(|action| action.interaction_id.clone());

        Ok(Self {
            surface_id,
            revision: revision.unwrap_or(0),
            bar: ContextBar {
                back,
                navigation: navigation_action,
                primary,
                secondary,
            },
            back_interaction_id,
            navigation_interaction_id,
            secondary_interaction_id,
            navigation_overlay: OverlaySpec {
                kind: OverlayKind::Navigation,
                title: Some(navigation_label.to_owned()),
                items: navigation_items,
            },
            secondary_overlay: OverlaySpec {
                kind: OverlayKind::ActionMenu,
                title: Some(secondary_label.to_owned()),
                items: secondary_items,
            },
            routes,
        })
    }
}

fn launcher_action(interaction_id: InteractionId, label: &str) -> ActionSpec {
    ActionSpec {
        interaction_id,
        label: label.to_owned(),
        accessibility_label: label.to_owned(),
        icon_token: None,
        enabled: true,
        tone: ActionTone::Standard,
        shortcut: None,
    }
}

fn action_spec(
    interaction_id: InteractionId,
    action: &ScreenAction,
    tone: ActionTone,
    shortcut: Option<StandardShortcut>,
) -> Result<ActionSpec, PresentationIdError> {
    Ok(ActionSpec {
        interaction_id,
        label: action.label.clone(),
        accessibility_label: action
            .a11y
            .as_ref()
            .and_then(|metadata| metadata.label.clone())
            .unwrap_or_else(|| action.label.clone()),
        icon_token: None,
        enabled: action.enabled,
        tone,
        shortcut,
    })
}

fn scoped_interaction(
    revision: Option<u64>,
    id: &str,
) -> Result<InteractionId, PresentationIdError> {
    InteractionId::new(match revision {
        Some(revision) => format!("surface.{revision}.context.{id}"),
        None => id.to_owned(),
    })
}

fn tone_for(action: &ScreenAction) -> ActionTone {
    if matches!(action.style, ActionStyle::Destructive) {
        ActionTone::Destructive
    } else {
        ActionTone::Standard
    }
}

fn validate_not_reserved(id: &str) -> Result<(), ContextualSurfaceError> {
    if id == BACK_INTERACTION_ID
        || id == NAVIGATION_INTERACTION_ID
        || id == SECONDARY_INTERACTION_ID
        || id.starts_with(NAVIGATION_ITEM_PREFIX)
    {
        Err(ContextualSurfaceError::ReservedInteractionId(id.to_owned()))
    } else {
        Ok(())
    }
}
