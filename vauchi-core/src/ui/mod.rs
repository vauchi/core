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
mod engine;
mod onboarding;
mod screen;

pub use action::{ActionResult, UserAction};
pub use component::{
    ActionListItem, Component, ContactItem, FieldDisplay, GroupCardView, InfoItem, InputType,
    SettingsItem, SettingsItemKind, Status, TextStyle, ToggleItem, UiFieldVisibility,
    VisibilityMode,
};
pub use engine::WorkflowEngine;
pub use onboarding::{FieldSetup, GroupSetup, OnboardingData, OnboardingEngine};
pub use screen::{ActionStyle, Progress, ScreenAction, ScreenModel};
