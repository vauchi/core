// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The screens Core shows before storage opens (vauchi/private#580,
//! ADR-043 Amendment 7 §3).
//!
//! A locked keychain asks for the platform prompt and retries; data no key
//! opens offers to start over and deletes nothing until the person
//! confirms. Opening and deleting are the caller's: this reducer only says
//! when to retry and when the person confirmed.

use vauchi_core::{Command, Event, SurfaceId};

use super::rejection;
use super::{
    ActionStyle, ContextualSurface, ContextualSurfaceRoute, PreparedSurface, PreparedSurfaceError,
    ScreenAction, ScreenModel, UserAction,
};
use crate::i18n::{Locale, get_string};

const UNLOCK: &str = "unlock";
const START_OVER: &str = "start_over";
const CONFIRM_START_OVER: &str = "confirm_start_over";
const BACK: &str = "go_back";

/// Why storage did not open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageLockReason {
    /// The keychain needs the person to authenticate or the device to unlock.
    Locked,
    /// No key opens the data: the keychain lost it, or none was ever stored.
    Unreadable,
}

/// What the caller does next.
#[derive(Clone, Debug, PartialEq)]
pub enum StorageLockStep {
    Commands(Vec<Command>),
    /// Try to open storage again.
    RetryOpen,
    /// The person confirmed deleting the unreadable data.
    StartOverConfirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    Locked,
    Unreadable,
    ConfirmStartOver,
}

pub struct StorageLockPresentation {
    screen: Screen,
    locale: Locale,
    revision: u64,
}

impl StorageLockPresentation {
    pub fn new(reason: StorageLockReason, locale: Locale) -> Self {
        Self {
            screen: screen_for(reason),
            locale,
            revision: 0,
        }
    }

    pub fn initial_commands(&mut self) -> Vec<Command> {
        self.render()
    }

    /// Shows `reason` after a retry did not open storage.
    pub fn show(&mut self, reason: StorageLockReason) -> Vec<Command> {
        self.screen = screen_for(reason);
        self.render()
    }

    pub fn dispatch(&mut self, event: Event) -> StorageLockStep {
        match event {
            Event::BiometricUnlockSucceeded if self.screen == Screen::Locked => {
                StorageLockStep::RetryOpen
            }
            Event::ActionActivated { .. } | Event::BackRequested { .. } => self
                .action_for(event)
                .map(|action| self.handle_action(&action))
                .unwrap_or_else(|| StorageLockStep::Commands(self.rejection())),
            _ => StorageLockStep::Commands(Vec::new()),
        }
    }

    fn handle_action(&mut self, action_id: &str) -> StorageLockStep {
        match (self.screen, action_id) {
            (Screen::Locked, UNLOCK) => StorageLockStep::RetryOpen,
            (Screen::Unreadable, START_OVER) => {
                self.screen = Screen::ConfirmStartOver;
                StorageLockStep::Commands(self.render())
            }
            (Screen::ConfirmStartOver, CONFIRM_START_OVER) => StorageLockStep::StartOverConfirmed,
            (Screen::ConfirmStartOver, BACK) => {
                self.screen = Screen::Unreadable;
                StorageLockStep::Commands(self.render())
            }
            _ => StorageLockStep::Commands(self.rejection()),
        }
    }

    /// The action an activation names, only if the current screen offers it.
    fn action_for(&self, event: Event) -> Option<String> {
        let screen = self.current_screen();
        let surface_id = SurfaceId::new(screen.screen_id.clone()).ok()?;
        let prepared =
            PreparedSurface::from_screen(surface_id.clone(), self.revision, &screen).ok()?;
        let route = match prepared.reduce(event.clone()) {
            Ok(action) => ContextualSurfaceRoute::UserAction(action),
            Err(PreparedSurfaceError::UnknownBinding) => self
                .contextual(surface_id, &screen)
                .ok()?
                .handle_event(event)
                .ok()?,
            Err(_) => return None,
        };
        match route {
            ContextualSurfaceRoute::UserAction(UserAction::ActionPressed { action_id }) => {
                Some(action_id)
            }
            ContextualSurfaceRoute::UserAction(UserAction::NavigateBack) => Some(BACK.into()),
            _ => None,
        }
    }

    fn render(&mut self) -> Vec<Command> {
        self.revision = self.revision.saturating_add(1);
        let screen = self.current_screen();
        let Ok(surface_id) = SurfaceId::new(screen.screen_id.clone()) else {
            return self.rejection();
        };
        let Ok(prepared) = PreparedSurface::from_screen(surface_id.clone(), self.revision, &screen)
        else {
            return self.rejection();
        };
        let Ok(contextual) = self.contextual(surface_id, &screen) else {
            return self.rejection();
        };
        let mut commands = vec![prepared.command()];
        commands.extend(contextual.initial_commands());
        if self.screen == Screen::Locked {
            commands.push(Command::RequestBiometricUnlock);
        }
        commands
    }

    fn contextual(
        &self,
        surface_id: SurfaceId,
        screen: &ScreenModel,
    ) -> Result<ContextualSurface, super::ContextualSurfaceError> {
        ContextualSurface::compose_revisioned(
            surface_id,
            self.revision,
            screen,
            &[],
            None,
            &self.t("nav.more"),
            &self.t("action_list.title"),
        )
    }

    /// Core's answer to event JSON that does not decode (ADR-045 Am1).
    pub fn reject_event_json(&self, error: &vauchi_core::EventJsonError) -> Vec<Command> {
        rejection::event_json_rejection(self.locale, error)
    }

    fn rejection(&self) -> Vec<Command> {
        rejection::dispatch_rejection(self.locale)
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    fn current_screen(&self) -> ScreenModel {
        let (id, title, body, action, back) = match self.screen {
            Screen::Locked => (
                "storage_lock.locked",
                "storage_lock.locked_title",
                "storage_lock.locked_body",
                (UNLOCK, "storage_lock.unlock"),
                false,
            ),
            Screen::Unreadable => (
                "storage_lock.unreadable",
                "storage_lock.unreadable_title",
                "storage_lock.unreadable_body",
                (START_OVER, "storage_lock.start_over"),
                false,
            ),
            Screen::ConfirmStartOver => (
                "storage_lock.confirm_start_over",
                "storage_lock.confirm_title",
                "storage_lock.confirm_body",
                (CONFIRM_START_OVER, "storage_lock.confirm_start_over"),
                true,
            ),
        };
        ScreenModel {
            screen_id: id.into(),
            title: self.t(title),
            subtitle: Some(self.t(body)),
            contextual_actions: vec![ScreenAction {
                id: action.0.into(),
                label: self.t(action.1),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            }],
            nav_actions: if back {
                vec![ScreenAction {
                    id: BACK.into(),
                    label: self.t("action.back"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                }]
            } else {
                Vec::new()
            },
            ..Default::default()
        }
    }
}

fn screen_for(reason: StorageLockReason) -> Screen {
    match reason {
        StorageLockReason::Locked => Screen::Locked,
        StorageLockReason::Unreadable => Screen::Unreadable,
    }
}
