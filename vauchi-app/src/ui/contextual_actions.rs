// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{ActionSpec, Command, ContextBar, Event, InteractionId, SurfaceId};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ContextualActionCoordinatorError {
    #[error("an undo action is already active")]
    UndoAlreadyActive,
    #[error("the replacement context has no active primary")]
    PrimaryMissing,
    #[error("the event targets a different surface")]
    SurfaceMismatch,
    #[error("event is not handled by the contextual action coordinator")]
    UnsupportedEvent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextualActionTransition {
    pub interaction_id: InteractionId,
    pub undo_requested: bool,
    pub action_id: Option<String>,
    pub commands: Vec<Command>,
}

#[derive(Clone, Debug)]
struct PendingUndo {
    interaction_id: InteractionId,
    action_id: String,
    undo: ActionSpec,
    original_primary: ActionSpec,
}

#[derive(Clone, Debug)]
pub struct ContextualActionCoordinator {
    surface_id: SurfaceId,
    revision: u64,
    bar: ContextBar,
    pending_undo: Option<PendingUndo>,
}

impl ContextualActionCoordinator {
    pub fn new(surface_id: SurfaceId, revision: u64, bar: ContextBar) -> Self {
        Self {
            surface_id,
            revision,
            bar,
            pending_undo: None,
        }
    }

    pub fn surface_id(&self) -> &SurfaceId {
        &self.surface_id
    }

    pub fn initial_commands(&self) -> Vec<Command> {
        vec![self.set_bar_command()]
    }

    pub fn offer_causal_undo(
        &mut self,
        cause: &InteractionId,
        undo: ActionSpec,
    ) -> Result<Vec<Command>, ContextualActionCoordinatorError> {
        let action_id = undo.interaction_id.as_str().to_owned();
        self.offer_causal_undo_routed(cause, undo, action_id)
    }

    pub fn offer_causal_undo_routed(
        &mut self,
        _cause: &InteractionId,
        undo: ActionSpec,
        action_id: String,
    ) -> Result<Vec<Command>, ContextualActionCoordinatorError> {
        if self.pending_undo.is_some() {
            return Err(ContextualActionCoordinatorError::UndoAlreadyActive);
        }
        let primary = self
            .bar
            .primary
            .as_ref()
            .filter(|action| action.enabled)
            .cloned()
            .ok_or(ContextualActionCoordinatorError::PrimaryMissing)?;

        self.pending_undo = Some(PendingUndo {
            interaction_id: undo.interaction_id.clone(),
            action_id,
            undo: undo.clone(),
            original_primary: primary,
        });
        self.bar.primary = Some(undo);
        Ok(vec![self.set_bar_command()])
    }

    pub fn rebase(
        &mut self,
        revision: u64,
        bar: ContextBar,
        undo_interaction_id: InteractionId,
    ) -> Result<(), ContextualActionCoordinatorError> {
        self.revision = revision;
        let Some(pending) = &mut self.pending_undo else {
            self.bar = bar;
            return Ok(());
        };
        let original_primary = bar
            .primary
            .as_ref()
            .filter(|action| action.enabled)
            .cloned()
            .ok_or(ContextualActionCoordinatorError::PrimaryMissing)?;
        pending.original_primary = original_primary;
        pending.interaction_id = undo_interaction_id.clone();
        pending.undo.interaction_id = undo_interaction_id;
        self.bar = bar;
        self.bar.primary = Some(pending.undo.clone());
        Ok(())
    }

    pub fn handle_event(
        &mut self,
        event: Event,
    ) -> Result<ContextualActionTransition, ContextualActionCoordinatorError> {
        let Event::ActionActivated {
            surface_id,
            interaction_id,
        } = event
        else {
            return Err(ContextualActionCoordinatorError::UnsupportedEvent);
        };
        if surface_id != self.surface_id {
            return Err(ContextualActionCoordinatorError::SurfaceMismatch);
        }

        let undo_requested = self
            .pending_undo
            .as_ref()
            .is_some_and(|pending| pending.interaction_id == interaction_id);
        let mut action_id = None;
        let commands = if undo_requested {
            let pending = self
                .pending_undo
                .take()
                .ok_or(ContextualActionCoordinatorError::UndoAlreadyActive)?;
            action_id = Some(pending.action_id);
            self.bar.primary = Some(pending.original_primary);
            vec![self.set_bar_command()]
        } else {
            Vec::new()
        };

        Ok(ContextualActionTransition {
            interaction_id,
            undo_requested,
            action_id,
            commands,
        })
    }

    fn set_bar_command(&self) -> Command {
        Command::SetContextBar {
            surface_id: self.surface_id.clone(),
            revision: self.revision,
            bar: Box::new(self.bar.clone()),
        }
    }
}
