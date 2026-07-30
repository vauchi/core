// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{
    AccessibilitySpec, ActionSpec, ActionTone, Command, Event, InteractionId, StandardShortcut,
    SurfaceId,
};

use super::{AppEngine, AppScreen};
use crate::ui::{
    ActionResult, ContextualSurface, ContextualSurfaceError, ContextualSurfaceRoute,
    PreparedSurface, PreparedSurfaceError, PresentationCoordinatorError, UserAction,
    WorkflowEngine,
};

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum AppPresentationError {
    #[error("presentation surface revision space is exhausted")]
    RevisionExhausted,
    #[error("legacy action result escaped the reducer boundary: {variant}")]
    UnresolvedActionResult { variant: &'static str },
    #[error("invalid contextual surface: {0}")]
    Contextual(#[from] ContextualSurfaceError),
    #[error("invalid responsive presentation event: {0}")]
    Responsive(#[from] PresentationCoordinatorError),
    #[error("invalid prepared presentation surface: {0}")]
    Prepared(#[from] PreparedSurfaceError),
    #[error("invalid contextual action transition: {0}")]
    ContextualAction(#[from] crate::ui::ContextualActionCoordinatorError),
    #[error("authentication transition failed: {0}")]
    Authentication(String),
}

impl AppEngine {
    /// Return the complete initial renderer state as one ordered command batch.
    pub fn initial_commands(&mut self) -> Result<Vec<Command>, AppPresentationError> {
        let screen = self.current_screen();
        let mut commands = self.surface_commands(&screen)?;
        commands.extend(self.drain_pending_commands());
        Ok(commands)
    }

    /// Reduce one raw shell event into the next ordered command batch.
    pub fn dispatch(&mut self, event: Event) -> Result<Vec<Command>, AppPresentationError> {
        if matches!(event, Event::PresentationInvalidated) {
            self.invalidate_all();
            return self.initial_commands();
        }

        if let Event::DeepLinkOpened { uri } = &event {
            return self.reduce_user_action(UserAction::LinkOpened { uri: uri.clone() }, None);
        }

        if matches!(event, Event::AppBackgrounded) {
            if self.handle_app_backgrounded().is_none() {
                return Ok(Vec::new());
            }
            let screen = self.current_screen();
            let mut commands = self.surface_commands(&screen)?;
            commands.extend(self.drain_pending_commands());
            return Ok(commands);
        }

        if matches!(
            event,
            Event::PresentationEnvironmentChanged { .. } | Event::SurfaceActivated { .. }
        ) {
            return Ok(self.presentation_coordinator.handle_event(event)?);
        }

        if !matches!(
            event,
            Event::ValueChanged { .. }
                | Event::ActionActivated { .. }
                | Event::BackRequested { .. }
                | Event::OverlayDismissed { .. }
        ) {
            return self.reduce_hardware_event(event);
        }

        let event_surface_id = presentation_event_surface_id(&event)
            .expect("interactive presentation events always carry a surface");
        self.presentation_coordinator
            .ensure_active_surface(event_surface_id)?;
        let (event_screen, screen, prepared) = self.prepared_visible_surface(event_surface_id)?;
        let cause = match &event {
            Event::ActionActivated {
                surface_id,
                interaction_id,
            } => Some((surface_id.clone(), interaction_id.clone())),
            _ => None,
        };
        if let Some(coordinator) = self.contextual_actions.get_mut(event_surface_id)
            && matches!(event, Event::ActionActivated { .. })
        {
            let transition = coordinator.handle_event(event.clone())?;
            if transition.undo_requested {
                let action_id = transition
                    .action_id
                    .ok_or(crate::ui::ContextualActionCoordinatorError::UndoAlreadyActive)?;
                return self.reduce_user_action(UserAction::UndoPressed { action_id }, None);
            }
        }

        let route = match &event {
            Event::ValueChanged { .. } => {
                ContextualSurfaceRoute::UserAction(prepared.reduce(event)?)
            }
            Event::ActionActivated { .. } => match prepared.reduce(event.clone()) {
                Ok(action) => ContextualSurfaceRoute::UserAction(action),
                Err(PreparedSurfaceError::UnknownBinding) => self
                    .contextual_surface_for_screen(event_surface_id.clone(), &screen)?
                    .handle_event(event)?,
                Err(error) => return Err(error.into()),
            },
            _ => self
                .contextual_surface_for_screen(event_surface_id.clone(), &screen)?
                .handle_event(event)?,
        };

        match route {
            ContextualSurfaceRoute::Commands(mut commands) => {
                commands.extend(self.drain_pending_commands());
                Ok(commands)
            }
            ContextualSurfaceRoute::UserAction(action) => {
                self.activate_visible_screen(event_screen);
                self.reduce_user_action(action, cause)
            }
        }
    }

    fn prepared_visible_surface(
        &self,
        requested_surface: &SurfaceId,
    ) -> Result<(AppScreen, crate::ui::ScreenModel, PreparedSurface), AppPresentationError> {
        let current_surface =
            SurfaceId::new(self.screen.screen_id()).map_err(ContextualSurfaceError::from)?;
        if &current_surface == requested_surface {
            let screen = self.current_screen();
            let prepared =
                PreparedSurface::from_screen(current_surface, self.surface_revision, &screen)?;
            return Ok((self.screen.clone(), screen, prepared));
        }
        if let Some(companion) = self.responsive_companion_surface()?
            && &companion.surface_id == requested_surface
        {
            return Ok((companion.screen, companion.model, companion.prepared));
        }
        Err(PreparedSurfaceError::SurfaceMismatch.into())
    }

    fn activate_visible_screen(&mut self, screen: AppScreen) {
        if self.screen != screen {
            self.activate_surface_engine(screen);
        }
    }

    fn contextual_surface_for_screen(
        &self,
        surface_id: SurfaceId,
        screen: &crate::ui::ScreenModel,
    ) -> Result<ContextualSurface, ContextualSurfaceError> {
        let locale = self.render_context.resolved_locale();
        ContextualSurface::compose_revisioned(
            surface_id,
            self.surface_revision,
            screen,
            &self.sidebar_items(locale),
            &self.t("nav.more"),
            &self.t("action_list.title"),
        )
    }

    fn surface_commands(
        &mut self,
        screen: &crate::ui::ScreenModel,
    ) -> Result<Vec<Command>, AppPresentationError> {
        let surface_id =
            SurfaceId::new(self.screen.screen_id()).map_err(ContextualSurfaceError::from)?;
        let prepared =
            PreparedSurface::from_screen(surface_id.clone(), self.surface_revision, screen)?;
        let contextual = self.contextual_surface_for_screen(surface_id.clone(), screen)?;
        let companion = self.responsive_companion_surface()?;
        let mut visible = vec![(surface_id.clone(), prepared, contextual)];
        if let Some(companion) = companion {
            let contextual =
                self.contextual_surface_for_screen(companion.surface_id.clone(), &companion.model)?;
            let entry = (companion.surface_id, companion.prepared, contextual);
            match companion.role {
                super::responsive_surfaces::CompanionRole::Primary => visible.insert(0, entry),
                super::responsive_surfaces::CompanionRole::Detail => visible.push(entry),
            }
        }
        self.presentation_coordinator.configure_surfaces(
            visible[0].0.clone(),
            visible.get(1).map(|entry| entry.0.clone()),
            surface_id,
        );
        for (surface_id, _, contextual) in &visible {
            self.rebase_contextual_actions(surface_id.clone(), contextual.context_bar().clone())?;
        }
        let mut commands = Vec::new();
        for (_, prepared, _) in &visible {
            commands.push(prepared.command());
        }
        for (surface_id, _, _) in &visible {
            if let Some(coordinator) = self.contextual_actions.get(surface_id) {
                commands.extend(coordinator.initial_commands());
            }
        }
        if let Some(profile) = self.presentation_coordinator.current_profile_command() {
            commands.push(profile);
        }
        Ok(commands)
    }

    fn reduce_user_action(
        &mut self,
        action: UserAction,
        cause: Option<(SurfaceId, InteractionId)>,
    ) -> Result<Vec<Command>, AppPresentationError> {
        let before = self.projected_visible_surface();
        let result = self.handle_action(action);
        if !self.preserves_binding_meaning(before.as_ref()) {
            self.surface_revision = self
                .surface_revision
                .checked_add(1)
                .ok_or(AppPresentationError::RevisionExhausted)?;
        }
        let screen = self.current_screen();
        let mut commands = self.surface_commands(&screen)?;
        commands.extend(self.offer_causal_undo(&result, cause.as_ref()));
        super::result_commands::append_result_commands(result, &mut commands)
            .map_err(|variant| AppPresentationError::UnresolvedActionResult { variant })?;
        commands.extend(self.drain_pending_commands());
        Ok(commands)
    }

    fn projected_visible_surface(&self) -> Option<(SurfaceId, PreparedSurface)> {
        let surface_id = SurfaceId::new(self.screen.screen_id()).ok()?;
        let screen = self.current_screen();
        let prepared =
            PreparedSurface::from_screen(surface_id.clone(), self.surface_revision, &screen)
                .ok()?;
        Some((surface_id, prepared))
    }

    /// Whether the ids already handed to shells still mean what they meant.
    ///
    /// Shells key their composition state on these ids, so reminting them for
    /// a content-only change rebuilds live widgets: on Android that drops
    /// focus and closes the soft keyboard between keystrokes. The revision
    /// therefore tracks binding topology, not content.
    fn preserves_binding_meaning(&self, before: Option<&(SurfaceId, PreparedSurface)>) -> bool {
        let Some((before_id, before_surface)) = before else {
            return false;
        };
        let Some((after_id, after_surface)) = self.projected_visible_surface() else {
            return false;
        };
        *before_id == after_id && before_surface.routes_match(&after_surface)
    }

    fn offer_causal_undo(
        &mut self,
        result: &ActionResult,
        cause: Option<&(SurfaceId, InteractionId)>,
    ) -> Vec<Command> {
        let (
            ActionResult::ShowToast {
                undo_action_id: Some(undo_action_id),
                undo_label: Some(undo_label),
                ..
            },
            Some((_, cause)),
        ) = (result, cause)
        else {
            return Vec::new();
        };
        let Ok(active_surface_id) = SurfaceId::new(self.screen.screen_id()) else {
            return Vec::new();
        };
        let Some(coordinator) = self.contextual_actions.get_mut(&active_surface_id) else {
            return Vec::new();
        };
        let Ok(interaction_id) =
            InteractionId::new(format!("surface.{}.context.undo", self.surface_revision))
        else {
            return Vec::new();
        };
        let undo = ActionSpec {
            interaction_id,
            label: undo_label.clone(),
            accessibility_label: AccessibilitySpec::label(undo_label).label,
            icon_token: None,
            enabled: true,
            tone: ActionTone::Standard,
            shortcut: Some(StandardShortcut::Undo),
        };
        coordinator
            .offer_causal_undo_routed(cause, undo, undo_action_id.clone())
            .unwrap_or_default()
    }

    fn rebase_contextual_actions(
        &mut self,
        surface_id: SurfaceId,
        bar: vauchi_core::ContextBar,
    ) -> Result<(), AppPresentationError> {
        let undo_interaction_id =
            InteractionId::new(format!("surface.{}.context.undo", self.surface_revision))
                .map_err(ContextualSurfaceError::from)?;
        if let Some(coordinator) = self.contextual_actions.get_mut(&surface_id) {
            coordinator.rebase(self.surface_revision, bar, undo_interaction_id)?;
        } else {
            self.contextual_actions.insert(
                surface_id.clone(),
                crate::ui::ContextualActionCoordinator::new(surface_id, self.surface_revision, bar),
            );
        }
        Ok(())
    }

    fn reduce_hardware_event(
        &mut self,
        event: Event,
    ) -> Result<Vec<Command>, AppPresentationError> {
        if matches!(event, Event::BiometricUnlockSucceeded) {
            let outcome = self
                .vauchi_mut()
                .biometric_unlock_check()
                .map_err(|error| AppPresentationError::Authentication(error.to_string()))?;
            let requirement = match outcome {
                vauchi_core::BiometricUnlockOutcome::Unlocked => {
                    vauchi_core::AuthenticationRequirement::Unlocked
                }
                vauchi_core::BiometricUnlockOutcome::PromptForDuressPin => {
                    vauchi_core::AuthenticationRequirement::AppPassword
                }
                _ => vauchi_core::AuthenticationRequirement::AppPassword,
            };
            return Ok(vec![Command::SetAuthenticationRequirement { requirement }]);
        }
        let next_revision = self
            .surface_revision
            .checked_add(1)
            .ok_or(AppPresentationError::RevisionExhausted)?;
        let result = self.handle_hardware_event(event);
        if result.is_some() {
            self.surface_revision = next_revision;
        }
        let screen = self.current_screen();
        let mut commands = self.surface_commands(&screen)?;
        if let Some(result) = result {
            super::result_commands::append_result_commands(result, &mut commands)
                .map_err(|variant| AppPresentationError::UnresolvedActionResult { variant })?;
        }
        commands.extend(self.drain_pending_commands());
        Ok(commands)
    }
}

fn presentation_event_surface_id(event: &Event) -> Option<&SurfaceId> {
    match event {
        Event::ActionActivated { surface_id, .. }
        | Event::ValueChanged { surface_id, .. }
        | Event::BackRequested { surface_id }
        | Event::OverlayDismissed { surface_id, .. } => Some(surface_id),
        _ => None,
    }
}

// INLINE_TEST_REQUIRED: revision exhaustion requires access to the reducer's
// private generation counter and cannot be induced through the public API.
#[cfg(test)]
mod tests {
    use super::*;
    use vauchi_core::api::Vauchi;

    // @internal
    #[test]
    fn reducer_fails_closed_when_surface_revision_is_exhausted() {
        let mut app = AppEngine::new(Vauchi::in_memory().expect("in-memory core"));
        app.surface_revision = u64::MAX;
        let commands = app.initial_commands().expect("initial commands");
        let (surface_id, interaction_id) = commands
            .iter()
            .find_map(|command| {
                let Command::SetContextBar {
                    surface_id, bar, ..
                } = command
                else {
                    return None;
                };
                bar.primary
                    .as_ref()
                    .map(|primary| (surface_id.clone(), primary.interaction_id.clone()))
            })
            .expect("primary action");

        let error = app
            .dispatch(Event::ActionActivated {
                surface_id,
                interaction_id,
            })
            .expect_err("revision exhaustion must fail closed");

        assert_eq!(error, AppPresentationError::RevisionExhausted);
    }
}
