// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{
    AccessibilitySpec, ActionSpec, ActionTone, Command, Event, EventJsonError, InteractionId,
    StandardShortcut, SurfaceId,
};

use super::{AppEngine, AppScreen, TabLayout};
use crate::ui::rejection;
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

impl AppPresentationError {
    /// The variant alone, safe to log where the `Display` text is not.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::RevisionExhausted => "revision_exhausted",
            Self::UnresolvedActionResult { .. } => "unresolved_action_result",
            Self::Contextual(_) => "contextual",
            Self::Responsive(_) => "responsive",
            Self::Prepared(_) => "prepared",
            Self::ContextualAction(_) => "contextual_action",
            Self::Authentication(_) => "authentication",
        }
    }
}

impl AppEngine {
    /// Turn a re-request for the already-open overlay into a dismissal.
    ///
    /// The context-bar affordances emit `PresentOverlay` unconditionally —
    /// `ContextualSurface` is rebuilt per event and cannot know what is on
    /// screen. Core holds that state instead, so activating the same
    /// affordance twice closes the overlay rather than re-presenting it.
    /// Asking for a *different* overlay still opens it: the user named
    /// which menu they want, so it must not resolve to a dismissal merely
    /// because some overlay was open.
    fn resolve_overlay_toggle(&mut self, commands: Vec<Command>) -> Vec<Command> {
        commands
            .into_iter()
            .map(|command| match command {
                Command::PresentOverlay {
                    surface_id,
                    revision,
                    overlay,
                } => {
                    let already_open = self.open_overlay.as_ref().is_some_and(|(open_id, open)| {
                        *open_id == surface_id && open.kind == overlay.kind
                    });
                    if already_open {
                        self.open_overlay = None;
                        Command::DismissOverlay {
                            surface_id,
                            revision,
                            kind: overlay.kind,
                        }
                    } else {
                        self.open_overlay = Some((surface_id.clone(), overlay.clone()));
                        Command::PresentOverlay {
                            surface_id,
                            revision,
                            overlay,
                        }
                    }
                }
                other => other,
            })
            .collect()
    }

    /// Close whatever overlay is open, because the user acted on something
    /// inside it.
    ///
    /// `resolve_overlay_toggle` only closes an overlay when the affordance
    /// that opened it is activated again; the shell only reports
    /// `OverlayDismissed` for a tap-outside or Close. Choosing an item —
    /// a navigation destination, an action-list entry — was neither, so
    /// the menu stayed on screen over the destination it had just opened.
    /// That presented as the destination "not working".
    ///
    /// Returns the command batch to prepend, empty when nothing is open.
    fn dismiss_open_overlay(&mut self) -> Vec<Command> {
        let Some((surface_id, overlay)) = self.open_overlay.take() else {
            return Vec::new();
        };
        vec![Command::DismissOverlay {
            surface_id,
            revision: self.surface_revision,
            kind: overlay.kind,
        }]
    }

    /// Forget the open overlay when the shell reports it dismissed itself
    /// (tap outside, Close). Without this the next activation would toggle
    /// closed against state that no longer matches the screen.
    fn clear_open_overlay(&mut self, event: &Event) {
        if let Event::OverlayDismissed { surface_id, kind } = event
            && self
                .open_overlay
                .as_ref()
                .is_some_and(|(open_id, open)| open_id == surface_id && open.kind == *kind)
        {
            self.open_overlay = None;
        }
    }

    /// Return the complete initial renderer state as one ordered command batch.
    pub fn initial_commands(&mut self) -> Result<Vec<Command>, AppPresentationError> {
        let screen = self.current_screen();
        let mut commands = self.surface_commands(&screen)?;
        commands.extend(self.reopened_overlay(&commands));
        commands.extend(self.drain_pending_commands());
        Ok(commands)
    }

    /// The overlay Core still records as open, re-presented on top of a
    /// rebuilt surface. A rebuild re-applies the surface, and the shell drops
    /// any overlay with it; without this, an invalidation closed open menus
    /// while Core kept believing them open (vauchi/private#9).
    fn reopened_overlay(&self, commands: &[Command]) -> Option<Command> {
        let (open_id, overlay) = self.open_overlay.as_ref()?;
        commands.iter().find_map(|command| match command {
            Command::ReplaceSurface { surface } if surface.surface_id == *open_id => {
                Some(Command::PresentOverlay {
                    surface_id: open_id.clone(),
                    revision: surface.revision,
                    overlay: overlay.clone(),
                })
            }
            _ => None,
        })
    }

    /// Presentation for an event the boundary decoder refused: a prepared
    /// alert, never the error value (ADR-045 Am1).
    pub fn reject_event_json(&self, error: &EventJsonError) -> Vec<Command> {
        rejection::event_json_rejection(self.render_context.resolved_locale(), error)
    }

    /// Presentation for an event `dispatch` rejected.
    ///
    /// Only the variant is logged: some variants carry text derived from
    /// the event, which may hold user input (logging rules: error types,
    /// never content).
    pub fn reject_dispatch(&self, error: &AppPresentationError) -> Vec<Command> {
        tracing::warn!(kind = error.kind(), "event rejected by dispatch");
        rejection::dispatch_rejection(self.render_context.resolved_locale())
    }

    /// Reduce one raw shell event into the next ordered command batch.
    pub fn dispatch(&mut self, event: Event) -> Result<Vec<Command>, AppPresentationError> {
        self.clear_open_overlay(&event);

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

        if let Event::SurfaceActivated { surface_id } = &event
            && self.presentation_coordinator.was_left_behind(surface_id)
        {
            tracing::info!("[Presentation] ignored an activation of a surface Core has left");
            return Ok(Vec::new());
        }
        if matches!(
            event,
            Event::PresentationEnvironmentChanged { .. } | Event::SurfaceActivated { .. }
        ) {
            let before = self.current_screen();
            let mut commands = self.presentation_coordinator.handle_event(event)?;
            // A window crossing the short threshold changes a fixed exchange
            // screen (#513); any other screen must not be sent again, or a
            // resize would drop its pending Undo.
            if self.current_screen() != before {
                commands.extend(self.initial_commands()?);
            }
            return Ok(commands);
        }

        if !matches!(
            event,
            Event::ValueChanged { .. }
                | Event::InputSubmitted { .. }
                | Event::InputFocusEnded { .. }
                | Event::ActionActivated { .. }
                | Event::BackRequested { .. }
                | Event::OverlayDismissed { .. }
        ) {
            return self.reduce_hardware_event(event);
        }

        let event_surface_id = presentation_event_surface_id(&event)
            .expect("interactive presentation events always carry a surface");
        if self
            .presentation_coordinator
            .was_left_behind(event_surface_id)
        {
            tracing::info!("[Presentation] ignored an event for a surface Core has left");
            return Ok(Vec::new());
        }
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
            Event::ValueChanged { binding_id, .. }
            | Event::InputSubmitted { binding_id, .. }
            | Event::InputFocusEnded { binding_id, .. } => {
                let binding_id = binding_id.clone();
                match prepared.reduce(event) {
                    Ok(action) => ContextualSurfaceRoute::UserAction(action),
                    Err(PreparedSurfaceError::UnknownBinding)
                        if prepared.is_stale_binding(&binding_id) =>
                    {
                        tracing::info!(
                            "[Presentation] dropped a value for stale binding {} (surface at revision {})",
                            binding_id.as_str(),
                            self.surface_revision
                        );
                        return Ok(Vec::new());
                    }
                    Err(PreparedSurfaceError::UnknownBinding)
                        if prepared.is_retired_capture_binding(&binding_id) =>
                    {
                        tracing::info!(
                            "[Presentation] dropped a decode for a camera no longer on screen"
                        );
                        return Ok(Vec::new());
                    }
                    Err(error) => return Err(error.into()),
                }
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
            ContextualSurfaceRoute::Commands(commands) => {
                let mut commands = self.resolve_overlay_toggle(commands);
                commands.extend(self.drain_pending_commands());
                Ok(commands)
            }
            ContextualSurfaceRoute::UserAction(action) => {
                self.activate_visible_screen(event_screen);
                let dismissal = self.dismiss_open_overlay();
                let mut commands = self.reduce_user_action(action, cause)?;
                // Prepended: the shell applies the batch in order, so the
                // menu must close before the destination renders, or it
                // covers the screen the user just chose.
                commands.splice(0..0, dismissal);
                Ok(commands)
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
            let prepared = PreparedSurface::from_screen_in(
                current_surface,
                self.surface_revision,
                &screen,
                self.render_context.resolved_locale(),
            )?;
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
        // A locked app publishes no destinations. Composing them anyway put a
        // navigation overlay on the lock surface and registered every entry as
        // a routed `NavigateToTab`, so two taps reached the whole app without
        // the password — and because Core supplies the bar, every shell
        // inherited it (2026-08-12-android-app-password-bypass). Dispatch
        // refuses the route as well; an affordance that is never offered and a
        // route that always refuses fail independently.
        let destinations = if self.is_locked() {
            Vec::new()
        } else {
            self.sidebar_items(locale)
        };
        // The persistent navigation reflects the app's overall current
        // destination, not the individual surface being composed: a
        // companion (detail) pane shares the same selection as the
        // primary pane, since there is one active destination at a time.
        let selected_tab = self.current_tab_id(TabLayout::Desktop);
        let information = self.screen_information(&surface_id, screen);
        let surface = ContextualSurface::compose_revisioned(
            surface_id,
            self.surface_revision,
            screen,
            &destinations,
            selected_tab,
            &self.t("nav.more"),
            &self.t("action_list.title"),
        )?
        .with_close_label(&self.t("action.close"));
        match information {
            Some(body) => surface.with_information(
                &self.t("context_bar.info"),
                &self.t("context_bar.info_a11y"),
                &body,
            ),
            None => Ok(surface),
        }
    }

    /// What the info slot says about `screen`: `screen_info.<screen_id>`,
    /// else `screen_info.<surface_id>`, in the active locale with the title
    /// standing in for `{name}`; nothing while the person has help icons
    /// off or the screen has no text yet (vauchi/private#479). A companion
    /// pane carries its engine's own screen id (`contact_list`) under the
    /// app's surface id (`contacts`), hence the second key.
    fn screen_information(
        &self,
        surface_id: &SurfaceId,
        screen: &crate::ui::ScreenModel,
    ) -> Option<String> {
        if !self.vauchi().config().show_help_icons {
            return None;
        }
        let locale = self.render_context.resolved_locale();
        [screen.screen_id.as_str(), surface_id.as_str()]
            .into_iter()
            .find_map(|id| crate::i18n::try_get_string(locale, &format!("screen_info.{id}")))
            .map(|text| text.replace("{name}", &screen.title))
    }

    /// A heartbeat replaced what is on screen (a Link session completed or
    /// timed out, a Hover frame moved on): hand the shell the new surface
    /// the way a reduced event would, ahead of whatever the advance already
    /// queued (a celebration must land on the completion screen, not the
    /// waiting one). Shells render only what core hands them.
    pub(super) fn re_present_after_heartbeat(&mut self) {
        let Some(next_revision) = self.surface_revision.checked_add(1) else {
            return;
        };
        self.surface_revision = next_revision;
        let screen = self.current_screen();
        match self.surface_commands(&screen) {
            Ok(commands) => {
                let queued: Vec<Command> = self.pending_commands.drain(..).collect();
                self.extend_pending_commands(commands);
                self.extend_pending_commands(queued);
            }
            Err(error) => tracing::warn!(?error, "heartbeat re-present failed"),
        }
    }

    fn surface_commands(
        &mut self,
        screen: &crate::ui::ScreenModel,
    ) -> Result<Vec<Command>, AppPresentationError> {
        let surface_id =
            SurfaceId::new(self.screen.screen_id()).map_err(ContextualSurfaceError::from)?;
        let prepared = PreparedSurface::from_screen_in(
            surface_id.clone(),
            self.surface_revision,
            screen,
            self.render_context.resolved_locale(),
        )?;
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
        for (surface_id, _, contextual) in &visible {
            if let Some(coordinator) = self.contextual_actions.get(surface_id) {
                commands.extend(coordinator.initial_commands());
            }
            // `SetNavigation` immediately follows `SetContextBar` for the
            // same surface: it comes from `contextual` directly rather than
            // the coordinator above, which only tracks the four contextual
            // roles for Undo substitution, not the persistent nav.
            commands.push(contextual.navigation_command());
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
        // Glance one-sided QR: the scan binding's text carries the scanned
        // OOB payload. Route it to `apply_glance_scan` so the scanner pins
        // the displayer's identity + exchange key + co-presence nonce; the
        // subsequent `BleDeviceDiscovered` of that identity then connects
        // (`handle_glance_discovery`). A malformed / expired QR is rejected
        // there and latches nothing — the exposure-closer for
        // `2026-06-10-ble-unauthenticated-peer-identity`. This match used to
        // live only in the retired UniFFI action seam; it belongs here so
        // the canonical envelope routes identically (ADR-066).
        if let UserAction::TextChanged {
            component_id,
            value,
        } = &action
            && component_id == crate::ui::GLANCE_SCAN_COMPONENT_ID
            && matches!(
                self.screen,
                AppScreen::BleExchange {
                    mode: vauchi_core::exchange::mode::ExchangeMode::Glance
                }
            )
        {
            // Dev instrumentation (vauchi/private#9 GL-6): the variant name
            // only — never the scanned payload.
            match self.apply_glance_scan(value) {
                Ok(()) => tracing::info!("[Glance] scan applied"),
                Err(error) => {
                    let debug = format!("{error:?}");
                    let kind = debug.split('(').next().unwrap_or("?");
                    tracing::info!("[Glance] scan rejected: {kind}");
                }
            }
        }
        // Hover / TapHoverShake / Glance-on-the-multi-stage-screen: the
        // capture node's text carries the frame the camera just decoded.
        // Route it into the AppEngine-owned machine before `handle_action`
        // so the screen this batch renders already reflects the new phase.
        // The engine itself is a pure projection and drops the action
        // (`MultiStageExchangeEngine::handle_action`), so without this the
        // decode is parsed and discarded —
        // `2026-08-18-hover-decodes-the-peer-qr-but-never-advances`.
        if let UserAction::TextChanged { component_id, .. } = &action {
            // Dev instrumentation (dev-logging only; no PII — the component id
            // and a length). A camera decode that never reaches the machine is
            // indistinguishable from one the machine rejected: on device 87
            // decoded DATA frames produced no receipt while 7 did, with
            // `decrypt_fail=0` proving they never arrived
            // (`2026-08-18-hover-transfer-stalls-on-the-last-chunk`). This line
            // is the seam between "the shell forwarded it" and "core acted".
            tracing::info!(
                "[MSX] text binding id={component_id} on_screen={}",
                matches!(self.screen, AppScreen::MultiStageExchange { .. })
            );
        }
        // Taken before the scan is applied: a frame that ends the exchange
        // removes the camera, and a snapshot taken after it compares the
        // new screen with itself. The surface then went out under the
        // revision that still had the camera, so the decodes still in flight
        // were refused as unknown, not dropped as stale (vauchi/private#438).
        let before = self.projected_visible_surface();
        if let UserAction::TextChanged {
            component_id,
            value,
        } = &action
            && component_id == crate::ui::MULTI_STAGE_PEER_SCAN_COMPONENT_ID
            && matches!(self.screen, AppScreen::MultiStageExchange { .. })
        {
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.apply_multi_stage_peer_scan(value);
        }
        let result = self.handle_action(action);
        // DeviceLinkConfirmManual/Deny/Retry are Core-internal signals:
        // their machine side effects already ran inside `handle_action`
        // (`dispatch_device_link_side_effects`), and the completing/expired
        // screen state they leave behind is fully rendered by
        // `surface_commands` below. They carry nothing a shell can render,
        // so they are consumed here rather than rejected as unresolved
        // command variants.
        let internally_consumed = matches!(
            result,
            ActionResult::DeviceLinkConfirmManual { .. }
                | ActionResult::DeviceLinkDeny
                | ActionResult::DeviceLinkRetry
        );
        if !self.preserves_binding_meaning(before.as_ref()) {
            self.surface_revision = self
                .surface_revision
                .checked_add(1)
                .ok_or(AppPresentationError::RevisionExhausted)?;
        }
        let screen = self.current_screen();
        let mut commands = self.surface_commands(&screen)?;
        commands.extend(self.offer_causal_undo(&result, cause.as_ref()));
        // Text about an item is an information overlay on the surface it
        // belongs to, like the bar's (vauchi/private#479); only without a
        // surface does it fall back to the alert the reducer maps it to.
        let result = match result {
            ActionResult::ShowInfoOverlay { title, body } => {
                match self.information_overlay_command(&title, &body) {
                    Some(overlay) => {
                        commands.push(overlay);
                        return Ok(self.finish_batch(commands));
                    }
                    None => ActionResult::ShowInfoOverlay { title, body },
                }
            }
            other => other,
        };
        if !internally_consumed {
            super::result_commands::append_result_commands(result, &mut commands)
                .map_err(|variant| AppPresentationError::UnresolvedActionResult { variant })?;
        }
        commands.extend(self.drain_pending_commands());
        Ok(commands)
    }

    fn information_overlay_command(&self, title: &str, body: &str) -> Option<Command> {
        let (surface_id, _) = self.projected_visible_surface()?;
        Some(Command::PresentOverlay {
            surface_id,
            revision: self.surface_revision,
            overlay: vauchi_core::OverlaySpec {
                kind: vauchi_core::OverlayKind::Information,
                title: Some(title.to_owned()),
                items: Vec::new(),
                body: Some(body.to_owned()),
                close_label: Some(self.t("action.close")),
            },
        })
    }

    fn finish_batch(&mut self, mut commands: Vec<Command>) -> Vec<Command> {
        commands.extend(self.drain_pending_commands());
        commands
    }

    fn projected_visible_surface(&self) -> Option<(SurfaceId, PreparedSurface)> {
        let surface_id = SurfaceId::new(self.screen.screen_id()).ok()?;
        let screen = self.current_screen();
        let prepared = PreparedSurface::from_screen_in(
            surface_id.clone(),
            self.surface_revision,
            &screen,
            self.render_context.resolved_locale(),
        )
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

    /// With a duress PIN set, biometrics alone cannot tell the real user from
    /// a coerced one, so the lock screen stays and takes the password
    /// (ADR-032); otherwise the app opens.
    fn leave_lock_after_biometric_unlock(
        &mut self,
        unlocked: bool,
    ) -> Result<Vec<Command>, AppPresentationError> {
        let next_revision = self
            .surface_revision
            .checked_add(1)
            .ok_or(AppPresentationError::RevisionExhausted)?;
        if unlocked {
            let screen = self.default_screen();
            self.navigate_to_internal(screen);
        } else {
            self.engine
                .apply_update(crate::ui::EngineUpdate::BiometricUnlockPassed);
        }
        self.surface_revision = next_revision;
        let screen = self.current_screen();
        let mut commands = self.surface_commands(&screen)?;
        commands.extend(self.drain_pending_commands());
        Ok(commands)
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
            let unlocked = requirement == vauchi_core::AuthenticationRequirement::Unlocked;
            let mut commands = vec![Command::SetAuthenticationRequirement { requirement }];
            if self.is_locked() {
                commands.extend(self.leave_lock_after_biometric_unlock(unlocked)?);
            }
            return Ok(commands);
        }
        // BLE handshake machine gate, shared by every envelope that can
        // carry a hardware event (typed UniFFI seam and canonical
        // dispatch_json alike). Runs before the regular reduce so a
        // terminal machine event flips the chrome this batch renders.
        if self.route_ble_hardware_event_to_machine(&event) {
            self.pending_ble_terminal_invalidation = true;
        }
        // A peer discovery on the BLE exchange screen builds the
        // AppEngine-owned handshake session, whichever envelope carried it.
        // Glance connects asymmetrically to the scanned peer (F1); every
        // other mode lets the advertised tiebreak token decide the role,
        // matching `BleExchangeFlow`'s connect decision. Idempotent.
        if let Event::BleDeviceDiscovered { id, adv_data, .. } = &event {
            match self.screen {
                AppScreen::BleExchange {
                    mode: vauchi_core::exchange::mode::ExchangeMode::Glance,
                } => self.handle_glance_discovery(id, adv_data),
                AppScreen::BleExchange { .. } => {
                    self.start_ble_handshake_on_discovery(adv_data);
                }
                _ => {}
            }
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
        | Event::InputSubmitted { surface_id, .. }
        | Event::InputFocusEnded { surface_id, .. }
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
