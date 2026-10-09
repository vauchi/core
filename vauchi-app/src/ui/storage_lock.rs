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
const TRY_AGAIN: &str = "try_again";
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
    /// The next locked render asks for the platform prompt. Set at start and
    /// after a tapped Unlock; never after a prompt-triggered retry, so a
    /// prompt that cannot unlock storage does not loop.
    prompt_on_render: bool,
    retry_from_tap: bool,
}

impl StorageLockPresentation {
    pub fn new(reason: StorageLockReason, locale: Locale) -> Self {
        Self {
            screen: screen_for(reason),
            locale,
            revision: 0,
            prompt_on_render: true,
            retry_from_tap: false,
        }
    }

    pub fn initial_commands(&mut self) -> Vec<Command> {
        self.render()
    }

    /// Shows `reason` after a retry did not open storage.
    pub fn show(&mut self, reason: StorageLockReason) -> Vec<Command> {
        self.screen = screen_for(reason);
        self.prompt_on_render = self.retry_from_tap;
        self.render()
    }

    pub fn dispatch(&mut self, event: Event) -> StorageLockStep {
        match event {
            Event::BiometricUnlockSucceeded if self.screen == Screen::Locked => {
                self.retry_from_tap = false;
                StorageLockStep::RetryOpen
            }
            Event::ActionActivated { .. } | Event::BackRequested { .. } => {
                match self.route(event) {
                    Some(Routed::Action(action)) => self.handle_action(&action),
                    Some(Routed::Commands(commands)) => StorageLockStep::Commands(commands),
                    None => StorageLockStep::Commands(self.rejection()),
                }
            }
            _ => StorageLockStep::Commands(Vec::new()),
        }
    }

    fn handle_action(&mut self, action_id: &str) -> StorageLockStep {
        match (self.screen, action_id) {
            (Screen::Locked, UNLOCK) => {
                self.retry_from_tap = true;
                StorageLockStep::RetryOpen
            }
            (Screen::Unreadable, TRY_AGAIN) => {
                self.retry_from_tap = false;
                StorageLockStep::RetryOpen
            }
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

    /// What an activation means on the current screen: one of its actions,
    /// or commands such as opening the action menu. `None` for anything the
    /// screen does not offer.
    fn route(&self, event: Event) -> Option<Routed> {
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
                Some(Routed::Action(action_id))
            }
            ContextualSurfaceRoute::UserAction(UserAction::NavigateBack) => {
                Some(Routed::Action(BACK.into()))
            }
            ContextualSurfaceRoute::Commands(commands) => Some(Routed::Commands(commands)),
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
        if self.screen == Screen::Locked && self.prompt_on_render {
            commands.push(Command::RequestBiometricUnlock);
        }
        self.prompt_on_render = false;
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
        let (id, title, body, action, menu_action, back) = match self.screen {
            Screen::Locked => (
                "storage_lock.locked",
                "storage_lock.locked_title",
                "storage_lock.locked_body",
                (UNLOCK, "storage_lock.unlock"),
                None,
                false,
            ),
            Screen::Unreadable => (
                "storage_lock.unreadable",
                "storage_lock.unreadable_title",
                "storage_lock.unreadable_body",
                (TRY_AGAIN, "storage_lock.try_again"),
                Some((START_OVER, "storage_lock.start_over")),
                false,
            ),
            Screen::ConfirmStartOver => (
                "storage_lock.confirm_start_over",
                "storage_lock.confirm_title",
                "storage_lock.confirm_body",
                (CONFIRM_START_OVER, "storage_lock.confirm_start_over"),
                None,
                true,
            ),
        };
        ScreenModel {
            screen_id: id.into(),
            title: self.t(title),
            subtitle: Some(self.t(body)),
            contextual_actions: std::iter::once(ScreenAction {
                id: action.0.into(),
                label: self.t(action.1),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            })
            .chain(menu_action.map(|(id, label)| ScreenAction {
                id: id.into(),
                label: self.t(label),
                style: ActionStyle::Destructive,
                enabled: true,
                a11y: None,
            }))
            .collect(),
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

enum Routed {
    Action(String),
    Commands(Vec<Command>),
}

fn screen_for(reason: StorageLockReason) -> Screen {
    match reason {
        StorageLockReason::Locked => Screen::Locked,
        StorageLockReason::Unreadable => Screen::Unreadable,
    }
}
