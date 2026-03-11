// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core-driven UI types.
//!
//! Core describes what to render via ScreenModel and Component types.
//! Frontends implement a component library and ScreenRenderer.
//! User interactions flow back as UserAction.

mod action;
mod app_engine;
mod backup_recovery;
mod component;
mod contact_detail;
mod contact_edit;
mod contact_limit;
mod contact_list;
mod contact_merge;
mod contact_visibility;
mod delivery;
mod device_linking;
mod duplicate_detection;
mod duress_pin;
mod emergency_shred;
mod engine;
mod exchange;
mod form_dialog;
mod gdpr;
mod group_detail;
mod groups_list;
mod help;
mod lock_screen;
mod my_info;
mod my_info_entry_detail;
mod onboarding;
mod recovery_status;
mod screen;
mod settings;
mod support;
mod sync_status;
mod tor_settings;

pub use action::{ActionResult, UserAction};
pub use app_engine::{AppEngine, AppScreen};
pub use backup_recovery::{BackupMode, BackupRecoveryEngine};
pub use component::{
    ActionListItem, Component, ContactItem, FieldDisplay, GroupCardView, InfoItem, InputType,
    QrMode, SettingsItem, SettingsItemKind, Status, TextStyle, ToggleItem, UiFieldVisibility,
    VisibilityMode,
};
pub use contact_detail::{
    ContactDetailEngine, ContactNotFoundEngine, ContactViewMode, SharedInfoView,
};
pub use contact_edit::{ContactEditEngine, EditableContact, EditableField};
pub use contact_limit::ContactLimitEngine;
pub use contact_list::ContactListEngine;
pub use contact_merge::{ContactMergeEngine, MergePreview};
pub use contact_visibility::ContactVisibilityEngine;
pub use delivery::{DeliveryItem, DeliveryStatusEngine};
pub use device_linking::DeviceLinkingEngine;
pub use duplicate_detection::{DuplicateDetectionEngine, DuplicatePair};
pub use duress_pin::{DuressConfig, DuressPinEngine};
pub use emergency_shred::EmergencyShredEngine;
pub use engine::WorkflowEngine;
pub use exchange::{ExchangeConfig, ExchangeEngine};
pub use form_dialog::{FormDialogEngine, FormDialogType};
pub use gdpr::GdprEngine;
pub use group_detail::GroupDetailEngine;
pub use groups_list::{GroupInfo, GroupsEngine, GroupsMode};
pub use help::{HelpEngine, HelpItem};
pub use lock_screen::LockScreenEngine;
pub use my_info::{MyInfoEngine, MyInfoGroupTab, MyInfoProgress, MyInfoViewMode, OwnFieldInfo};
pub use my_info_entry_detail::{EntryContactInfo, MyInfoEntryDetailEngine};
pub use onboarding::{FieldSetup, GroupSetup, OnboardingData, OnboardingEngine};
pub use recovery_status::RecoveryEngine;
pub use screen::{ActionStyle, Progress, ScreenAction, ScreenModel};
pub use settings::{SettingsConfig, SettingsEngine};
pub use support::SupportEngine;
pub use sync_status::SyncStatusEngine;
pub use tor_settings::TorSettingsEngine;
