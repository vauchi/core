// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core-driven UI types.
//!
//! Core describes what to render via ScreenModel and Component types.
//! Frontends implement a component library and ScreenRenderer.
//! User interactions flow back as UserAction.

mod action;
mod activity_log;
#[cfg(feature = "network-rustls")]
mod app_engine;
mod archived_contacts;
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
mod device_management;
mod device_replacement;
mod duplicate_detection;
mod duress_pin;
mod emergency_shred;
mod engine;
mod exchange;
mod exchange_ble;
mod exchange_field_preview;
mod exchange_link;
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
pub mod reciprocity_confirmer;
mod recovery_status;
mod screen;
mod settings;
mod support;
#[cfg(feature = "network-rustls")]
mod sync_status;
pub use action::{ActionResult, UserAction};
pub use activity_log::{ActivityLogEngine, ActivityLogItem};
#[cfg(feature = "network-rustls")]
pub use app_engine::{AppEngine, AppScreen};
pub use archived_contacts::ArchivedContactsEngine;
pub use backup_recovery::{BackupMode, BackupRecoveryEngine};
pub use component::{
    A11y, AccessibilityRole, ActionListItem, Component, ContactItem, DropdownOption, FieldDisplay,
    GroupCardView, InfoItem, InputType, QrMode, SettingsItem, SettingsItemKind, Status, TextStyle,
    ToggleItem, UiFieldVisibility, VisibilityMode,
};
pub use contact_detail::{
    ContactDetailEngine, ContactNotFoundEngine, ContactViewMode, DeliverySummary, SharedInfoView,
};
pub use contact_edit::{ContactEditEngine, EditableContact, EditableField};
pub use contact_limit::ContactLimitEngine;
pub use contact_list::ContactListEngine;
pub use contact_merge::{ContactMergeEngine, MergePreview};
pub use contact_visibility::ContactVisibilityEngine;
pub use delivery::{DeliveryItem, DeliveryStatusEngine};
pub use device_linking::DeviceLinkingEngine;
pub use device_management::{DeviceListItem, DeviceManagementEngine};
pub use device_replacement::{CompletionOutcome, DeviceReplacementEngine, ReplacementRole};
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
pub use lock_screen::{DEFAULT_LOCK_MAX_ATTEMPTS, LockScreenEngine};
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
#[cfg(feature = "network-rustls")]
pub use sync_status::SyncStatusEngine;

/// Map a [`VauchiEvent`] to the screen IDs it invalidates.
///
/// This is the **single source of truth** for event→screen mapping.
/// All consumers (vauchi-platform, vauchi-cabi, linux-gtk, etc.) must call
/// this function rather than maintaining their own copy.
///
/// Returns an empty slice for events that don't affect any screen.
///
/// `VauchiEvent` is `#[non_exhaustive]` — unknown future variants return
/// an empty slice. When adding a new `VauchiEvent` variant, update this
/// function to include the relevant screen IDs.
#[cfg(feature = "network-rustls")]
pub fn affected_screens(event: &vauchi_core::api::VauchiEvent) -> Vec<&'static str> {
    use vauchi_core::api::VauchiEvent;

    match event {
        VauchiEvent::ContactAdded { .. }
        | VauchiEvent::ContactUpdated { .. }
        | VauchiEvent::ContactRemoved { .. }
        | VauchiEvent::ContactHidden { .. }
        | VauchiEvent::ContactUnhidden { .. }
        | VauchiEvent::ContactBlocked { .. }
        | VauchiEvent::ContactUnblocked { .. }
        | VauchiEvent::ContactSoftDeleted { .. }
        | VauchiEvent::ContactArchived { .. }
        | VauchiEvent::ContactUnarchived { .. } => {
            vec!["contacts", "contact_detail"]
        }
        VauchiEvent::OwnCardUpdated { .. } => vec!["my_info"],
        VauchiEvent::SyncStateChanged { .. }
        | VauchiEvent::SyncProgress { .. }
        | VauchiEvent::LabelSyncCompleted { .. } => {
            vec!["sync", "contacts"]
        }
        VauchiEvent::MessageDelivered { .. }
        | VauchiEvent::MessageFailed { .. }
        | VauchiEvent::DeliveryStatusUpdate { .. }
        | VauchiEvent::PreExpiryWarning { .. } => {
            vec!["delivery_status"]
        }
        VauchiEvent::ConnectionStateChanged { .. }
        | VauchiEvent::RelayHealthChanged { .. }
        | VauchiEvent::RelayFailover { .. } => {
            vec!["sync"]
        }
        VauchiEvent::IncomingUpdate { .. } => {
            vec!["contacts", "contact_detail"]
        }
        VauchiEvent::VisibilityChanged { .. } => {
            vec!["my_info", "contacts"]
        }
        VauchiEvent::EmergencyAlertReceived { .. } | VauchiEvent::EmergencyBroadcastSent { .. } => {
            vec!["contacts"]
        }
        VauchiEvent::DowngradeDetected { .. } | VauchiEvent::Error { .. } => vec![],
        // #[non_exhaustive] — unknown future variants don't invalidate screens.
        _ => vec![],
    }
}
