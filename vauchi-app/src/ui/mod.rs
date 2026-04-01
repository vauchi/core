// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core-driven UI types.
//!
//! Core describes what to render via ScreenModel and Component types.
//! Frontends implement a component library and ScreenRenderer.
//! User interactions flow back as UserAction.

mod action;
#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
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
mod exchange_mode_selection;
mod exchange_qr;
mod fingerprint_verify;
mod form_dialog;
mod gdpr;
mod group_detail;
mod groups_list;
mod help;
mod lock_screen;
mod more;
mod my_info;
mod my_info_entry_detail;
mod onboarding;
mod recovery_status;
mod screen;
mod settings;
mod support;
#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
mod sync_status;
pub use action::{ActionResult, UserAction};
#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
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
pub use fingerprint_verify::{FingerprintVerifyEngine, VerifyAction};
pub use form_dialog::{FormDialogEngine, FormDialogType};
pub use gdpr::GdprEngine;
pub use group_detail::GroupDetailEngine;
pub use groups_list::{GroupInfo, GroupsEngine, GroupsMode};
pub use help::{HelpEngine, HelpItem};
pub use lock_screen::LockScreenEngine;
pub use more::MoreEngine;
pub use my_info::{MyInfoEngine, MyInfoGroupTab, MyInfoProgress, MyInfoViewMode, OwnFieldInfo};
pub use my_info_entry_detail::{EntryContactInfo, MyInfoEntryDetailEngine};
pub use onboarding::{FieldSetup, GroupSetup, OnboardingData, OnboardingEngine};
pub use recovery_status::RecoveryEngine;
pub use screen::{
    ActionStyle, CURRENT_SCHEMA_VERSION, Progress, ScreenAction, ScreenModel, TabInfo,
};
pub use settings::{SettingsConfig, SettingsEngine};
pub use support::SupportEngine;
#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
pub use sync_status::SyncStatusEngine;
