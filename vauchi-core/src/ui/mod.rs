// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core-driven UI types.
//!
//! Core describes what to render via ScreenModel and Component types.
//! Frontends implement a component library and ScreenRenderer.
//! User interactions flow back as UserAction.

mod action;
mod component;
mod contact_edit;
mod contact_list;
mod delivery;
mod engine;
mod help;
mod home;
mod lock_screen;
mod onboarding;
mod screen;
mod settings;

pub use action::{ActionResult, UserAction};
pub use component::{
    ActionListItem, Component, ContactItem, FieldDisplay, GroupCardView, InfoItem, InputType,
    QrMode, SettingsItem, SettingsItemKind, Status, TextStyle, ToggleItem, UiFieldVisibility,
    VisibilityMode,
};
pub use contact_edit::{ContactEditEngine, EditableContact, EditableField};
pub use contact_list::ContactListEngine;
pub use delivery::{DeliveryItem, DeliveryStatusEngine};
pub use engine::WorkflowEngine;
pub use help::{HelpEngine, HelpItem};
pub use home::{HomeEngine, HomeProgress};
pub use lock_screen::LockScreenEngine;
pub use onboarding::{FieldSetup, GroupSetup, OnboardingData, OnboardingEngine};
pub use screen::{ActionStyle, Progress, ScreenAction, ScreenModel};
pub use settings::{SettingsConfig, SettingsEngine};
