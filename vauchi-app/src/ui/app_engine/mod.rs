// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Top-level application orchestrator.
//!
//! `AppEngine` wraps `Vauchi`, owns the active workflow engine,
//! handles navigation routing, and implements `WorkflowEngine` so
//! frontends see a single uniform interface.

mod intercept;
mod navigation;
mod routing;
mod screens;

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use vauchi_core::api::{HandlerId, Vauchi, VauchiEvent};
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::version::{
    APP_COMPAT_VERSION, AppUpdateStatus, VersionPolicy, unix_secs_to_date_string,
};

use crate::activity_log_writer::ActivityLogWriter;
use crate::notification_emitter::NotificationEmitter;
use crate::notification_types::{ActivityLogEntry, NotificationPreferences, PendingNotification};

use super::action::{ActionResult, UserAction};
use super::component::{Component, TextStyle};
use super::device_linking::DeviceLinkingEngine;
use super::engine::WorkflowEngine;
use super::screen::{ActionStyle, ScreenAction, ScreenModel};

/// Shared action ID for the update link button/banner.
const ACTION_OPEN_UPDATE_LINK: &str = "open_update_link";

/// Top-level screens in the application.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AppScreen {
    Onboarding,
    MyInfo,
    Contacts,
    ContactDetail {
        contact_id: String,
    },
    ContactEdit {
        contact_id: String,
    },
    ContactVisibility {
        contact_id: String,
    },
    Exchange,
    Settings,
    Help,
    Backup,
    Lock,
    DeviceLinking,
    DeviceManagement,
    DuressPin,
    EmergencyShred,
    DeliveryStatus,
    Sync,
    Recovery,
    Groups,
    GroupDetail {
        group_id: String,
    },
    Privacy,
    Support,
    FormDialog {
        dialog_type: super::form_dialog::FormDialogType,
    },
    MyInfoEntryDetail {
        field_id: String,
    },
    ContactDuplicates,
    ContactMerge {
        primary_name: String,
        primary_fields: Vec<String>,
        secondary_name: String,
        secondary_fields: Vec<String>,
    },
    ContactLimit,
    VerifyFingerprint {
        contact_id: String,
    },
    More,
    ActivityLog,
}

impl AppScreen {
    /// Canonical navigation-level string ID for this screen.
    ///
    /// Used by CABI to convert between `AppScreen` and the string IDs
    /// that frontends pass to `navigate_to` / receive from `available_screens`.
    /// Exhaustive — adding a new variant without a mapping is a compile error.
    pub fn screen_id(&self) -> &'static str {
        match self {
            Self::Onboarding => "onboarding",
            Self::MyInfo => "my_info",
            Self::Contacts => "contacts",
            Self::ContactDetail { .. } => "contact_detail",
            Self::ContactEdit { .. } => "contact_edit",
            Self::ContactVisibility { .. } => "contact_visibility",
            Self::Exchange => "exchange",
            Self::Settings => "settings",
            Self::Help => "help",
            Self::Backup => "backup",
            Self::Lock => "lock",
            Self::DeviceLinking => "device_linking",
            Self::DeviceManagement => "device_management",
            Self::DuressPin => "duress_pin",
            Self::EmergencyShred => "emergency_shred",
            Self::DeliveryStatus => "delivery_status",
            Self::Sync => "sync",
            Self::Recovery => "recovery",
            Self::Groups => "groups",
            Self::GroupDetail { .. } => "group_detail",
            Self::Privacy => "privacy",
            Self::Support => "support",
            Self::FormDialog { .. } => "form_dialog",
            Self::MyInfoEntryDetail { .. } => "entry_detail",
            Self::ContactDuplicates => "contact_duplicates",
            Self::ContactMerge { .. } => "contact_merge",
            Self::ContactLimit => "contact_limit",
            Self::VerifyFingerprint { .. } => "verify_fingerprint",
            Self::More => "more",
            Self::ActivityLog => "activity_log",
        }
    }

    /// Parse a navigation-level screen ID string into an `AppScreen`.
    ///
    /// Only handles simple (non-parameterized) screens. Parameterized screens
    /// like `ContactDetail` require additional data and return `None`.
    pub fn from_screen_id(id: &str) -> Option<Self> {
        Some(match id {
            "onboarding" => Self::Onboarding,
            "home" | "my_info" => Self::MyInfo,
            "contacts" => Self::Contacts,
            "exchange" => Self::Exchange,
            "settings" => Self::Settings,
            "help" => Self::Help,
            "backup" => Self::Backup,
            "lock" => Self::Lock,
            "device_linking" => Self::DeviceLinking,
            "duress_pin" => Self::DuressPin,
            "emergency_shred" => Self::EmergencyShred,
            "delivery_status" => Self::DeliveryStatus,
            "sync" => Self::Sync,
            "recovery" => Self::Recovery,
            "groups" => Self::Groups,
            "privacy" => Self::Privacy,
            "support" => Self::Support,
            "contact_duplicates" => Self::ContactDuplicates,
            "contact_limit" => Self::ContactLimit,
            "more" => Self::More,
            "activity_log" => Self::ActivityLog,
            _ => return None,
        })
    }
}

/// Tracks which contact undo is pending (delete or archive).
///
/// The `contact_id` field exists for debugging and potential future use
/// (e.g. confirming the undo matches the expected contact).
#[derive(Clone, Debug)]
pub(super) enum PendingContactUndo {
    SoftDelete {
        #[allow(dead_code)]
        contact_id: String,
    },
    Archive {
        #[allow(dead_code)]
        contact_id: String,
    },
}

/// Unified orchestrator for all frontends.
pub struct AppEngine {
    vauchi: Vauchi,
    screen: AppScreen,
    engine: Box<dyn WorkflowEngine>,
    engine_cache: HashMap<AppScreen, Box<dyn WorkflowEngine>>,
    /// Captured from onboarding TextChanged events for identity persistence.
    pending_display_name: Option<String>,
    /// Navigation history stack for back-button support.
    nav_history: Vec<AppScreen>,
    /// Field pending undo after delete from MyInfoEntryDetail.
    pending_field_undo: Option<(String, vauchi_core::contact_card::ContactField)>,
    /// Contact pending undo after soft-delete or archive.
    pending_contact_undo: Option<PendingContactUndo>,
    /// Cached field type catalog (built once from SocialNetworkRegistry).
    field_catalog: vauchi_core::contact_card::FieldTypeCatalog,
    /// Transient preview-as state — contact ID being previewed (not serialized).
    pub(super) preview_as_contact: Option<String>,
    /// Device hardware capabilities reported by the frontend at startup.
    /// Used to determine exchange mode availability.
    pub(super) device_capabilities: DeviceCapabilities,
    /// Current app update status from the relay/CDN version policy.
    update_status: AppUpdateStatus,
    /// Whether the user has dismissed the "update available" banner.
    update_dismissed: bool,
    /// Last timestamp (seconds) when activity log was polled for OS notifications.
    last_poll_time: u64,
    /// Channel receiver for events to be persisted to the activity log.
    event_rx: mpsc::Receiver<VauchiEvent>,
    /// Active event handler ID, used to unregister on drop.
    _event_handler_id: HandlerId,
}

impl AppEngine {
    /// Returns a reference to the inner Vauchi instance.
    pub fn vauchi(&self) -> &Vauchi {
        &self.vauchi
    }

    /// Returns a mutable reference to the inner Vauchi instance.
    pub fn vauchi_mut(&mut self) -> &mut Vauchi {
        &mut self.vauchi
    }

    /// Set device hardware capabilities (reported by frontend at startup).
    ///
    /// Invalidates the exchange screen cache so mode availability is
    /// recalculated on next visit.
    pub fn set_device_capabilities(&mut self, caps: DeviceCapabilities) {
        self.device_capabilities = caps;
        self.engine_cache.remove(&AppScreen::Exchange);
    }

    pub fn new(vauchi: Vauchi) -> Self {
        let screen = if !vauchi.has_identity() {
            AppScreen::Onboarding
        } else if vauchi.is_password_enabled().unwrap_or(false) {
            AppScreen::Lock
        } else {
            AppScreen::MyInfo
        };
        let caps = DeviceCapabilities::default();
        let engine = Self::create_engine(&vauchi, &screen, None, &caps);
        let registry = vauchi_core::social::SocialNetworkRegistry::with_defaults();
        let field_catalog = vauchi_core::contact_card::FieldTypeCatalog::new(&registry);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Register a permanent event handler that sends VauchiEvents to a channel
        // for deferred persistence to the activity log (ADR-031).
        // Since VauchiEvent handler must be Sync but mpsc::Sender is not Sync,
        // we wrap it in a Mutex.
        let (event_tx, event_rx) = mpsc::channel();
        let event_tx = std::sync::Mutex::new(event_tx);
        let event_handler_id = vauchi.add_event_handler(std::sync::Arc::new(move |event| {
            if let Ok(tx) = event_tx.lock() {
                let _ = tx.send(event);
            }
        }));

        Self {
            vauchi,
            screen,
            engine,
            engine_cache: HashMap::new(),
            pending_display_name: None,
            nav_history: Vec::new(),
            pending_field_undo: None,
            pending_contact_undo: None,
            field_catalog,
            preview_as_contact: None,
            device_capabilities: DeviceCapabilities::default(),
            update_status: AppUpdateStatus::UpToDate,
            update_dismissed: false,
            last_poll_time: now,
            event_rx,
            _event_handler_id: event_handler_id,
        }
    }

    /// Enter preview-as mode: show MyInfo as seen by the given contact.
    ///
    /// Sets transient state, invalidates the MyInfo cache, and navigates to MyInfo
    /// in PreviewAs view mode. The state is cleared by handling "exit-preview".
    pub fn preview_as(&mut self, contact_id: String) -> ScreenModel {
        self.preview_as_contact = Some(contact_id);
        self.invalidate_screen(&AppScreen::MyInfo);
        self.navigate_to(AppScreen::MyInfo)
    }

    /// Drain pending OS notifications.
    ///
    /// Processes buffered events through [`ActivityLogWriter`] and
    /// [`NotificationEmitter`], returning notifications for the frontend
    /// to display. Each call clears the buffer, so notifications are
    /// never returned twice.
    ///
    /// Frontends should call this after receiving an event callback.
    pub fn drain_pending_notifications(&mut self) -> Vec<PendingNotification> {
        let new_entries = self.drain_events_to_log();
        if new_entries.is_empty() {
            return Vec::new();
        }

        let prefs = NotificationPreferences::default();
        NotificationEmitter::evaluate(&new_entries, &prefs, |contact_id| {
            self.vauchi
                .get_contact(contact_id)
                .ok()
                .flatten()
                .map(|c| c.display_name().to_string())
                .unwrap_or_else(|| format!("Contact {}", &contact_id[..8.min(contact_id.len())]))
        })
    }

    pub fn current_app_screen(&self) -> &AppScreen {
        &self.screen
    }

    pub fn has_identity(&self) -> bool {
        self.vauchi.has_identity()
    }

    /// Notify core that the app was backgrounded (or is about to resume).
    ///
    /// If a password is set and the app is not already locked or in onboarding,
    /// navigates to the lock screen and returns the lock `ScreenModel`.
    /// Returns `None` if no action is needed (no password, already locked,
    /// or onboarding).
    ///
    /// Frontends should call this on app lifecycle events:
    /// - iOS: `scenePhase == .background`
    /// - Android: `onPause()` or `Lifecycle.Event.ON_STOP`
    /// - Desktop: window focus lost (optional, configurable)
    pub fn handle_app_backgrounded(&mut self) -> Option<ScreenModel> {
        // Don't lock during onboarding (no identity yet) or if already locked
        if matches!(self.screen, AppScreen::Lock | AppScreen::Onboarding) {
            return None;
        }
        // Only lock if a password is set
        if !self.vauchi.is_password_enabled().unwrap_or(false) {
            return None;
        }
        Some(self.navigate_to(AppScreen::Lock))
    }

    /// Signal that a peer device has connected during device linking.
    ///
    /// Transitions the `DeviceLinkingEngine` from `ShowQr` to `VerifyCode`.
    /// Returns the updated screen model, or `None` if the engine is not on
    /// the device linking screen.
    pub fn device_link_peer_connected(&mut self, verification_code: String) -> Option<ScreenModel> {
        if self.screen != AppScreen::DeviceLinking {
            return None;
        }
        let dl = self
            .engine
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<DeviceLinkingEngine>())?;
        dl.peer_connected(verification_code);
        Some(dl.current_screen())
    }

    /// Signal that data sync has completed during device linking.
    ///
    /// Transitions the `DeviceLinkingEngine` from `Syncing` to `Complete`.
    /// Returns the updated screen model, or `None` if the engine is not on
    /// the device linking screen.
    pub fn device_link_sync_complete(&mut self) -> Option<ScreenModel> {
        if self.screen != AppScreen::DeviceLinking {
            return None;
        }
        let dl = self
            .engine
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<DeviceLinkingEngine>())?;
        dl.sync_complete();
        Some(dl.current_screen())
    }

    /// Update the app's version status from a relay/CDN version policy.
    ///
    /// Evaluates the policy against the current `APP_COMPAT_VERSION` and
    /// resets the dismissed flag if an update becomes required.
    /// Current time as unix seconds — factored out for testability.
    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    pub fn set_version_policy(&mut self, policy: &VersionPolicy) {
        self.update_status = policy.evaluate(APP_COMPAT_VERSION, Self::now_secs());
        if matches!(self.update_status, AppUpdateStatus::UpdateRequired { .. }) {
            self.update_dismissed = false;
        }
    }

    /// Modify a `ScreenModel` to inject update banners or replace with a blocking screen.
    ///
    /// - `UpToDate` → no change
    /// - `UpdateAvailable` + not dismissed → dismissible banner at top
    /// - `UpdateRequired` with active deadline → non-dismissible banner at top
    /// - `UpdateRequired` with expired deadline → full blocking screen
    fn apply_update_overlay(&self, mut screen: ScreenModel) -> ScreenModel {
        match &self.update_status {
            AppUpdateStatus::UpToDate => screen,
            AppUpdateStatus::UpdateAvailable => {
                if self.update_dismissed {
                    return screen;
                }
                screen.components.insert(
                    0,
                    Component::Banner {
                        text: "A new version is available.".into(),
                        action_label: "Update".into(),
                        action_id: ACTION_OPEN_UPDATE_LINK.into(),
                    },
                );
                screen
            }
            AppUpdateStatus::UpdateRequired {
                grace_deadline: Some(deadline),
            } => {
                let date = unix_secs_to_date_string(*deadline);
                screen.components.insert(
                    0,
                    Component::Banner {
                        text: format!("Update required by {date}."),
                        action_label: "Update".into(),
                        action_id: ACTION_OPEN_UPDATE_LINK.into(),
                    },
                );
                screen
            }
            AppUpdateStatus::UpdateRequired {
                grace_deadline: None,
            } => ScreenModel::new(
                "update_required",
                "Update Required",
                vec![Component::Text {
                    id: "update_message".into(),
                    content: "This version is no longer supported. Please update to continue using the app.".into(),
                    style: TextStyle::Body,
                }],
                vec![ScreenAction {
                    id: ACTION_OPEN_UPDATE_LINK.into(),
                    label: "Update Now".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                }],
            ),
        }
    }
}

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

// INLINE_TEST_REQUIRED: initials() is module-private, cannot be tested from external tests/
#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn initials_single_word() {
        assert_eq!(initials("Alice"), "A");
    }

    #[test]
    fn initials_two_words() {
        assert_eq!(initials("Alice Smith"), "AS");
    }

    #[test]
    fn initials_three_words_takes_first_two() {
        assert_eq!(initials("Alice B Smith"), "AB");
    }

    #[test]
    fn initials_empty_string() {
        assert_eq!(initials(""), "");
    }

    #[test]
    fn initials_unicode() {
        assert_eq!(initials("Ägidius Ölmann"), "ÄÖ");
    }

    #[test]
    fn initials_extra_whitespace() {
        assert_eq!(initials("  Alice   Smith  "), "AS");
    }
}

// INLINE_TEST_REQUIRED: initials() is module-private, cannot be tested from external tests/
#[cfg(test)]
mod proptests {
    use super::initials;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn initials_never_panics(name in "\\PC*") {
            let result = initials(&name);
            // Unicode to_uppercase() can expand a single char to multiple,
            // so we only assert the result is valid UTF-8 (which String guarantees)
            // and that it equals its own uppercase form.
            prop_assert_eq!(result.clone(), result.to_uppercase());
        }

        #[test]
        fn initials_are_uppercase(name in "[a-z]+ [a-z]+") {
            let result = initials(&name);
            prop_assert_eq!(result.clone(), result.to_uppercase());
        }
    }
}

impl WorkflowEngine for AppEngine {
    fn current_screen(&self) -> ScreenModel {
        let screen = self.engine.current_screen();
        self.apply_update_overlay(screen)
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        self.drain_events_to_log();

        // Handle update link action from banner/button presses
        if matches!(
            &action,
            UserAction::ActionPressed { action_id } if action_id == ACTION_OPEN_UPDATE_LINK
        ) {
            if matches!(self.update_status, AppUpdateStatus::UpdateAvailable) {
                self.update_dismissed = true;
            }
            return ActionResult::OpenUrl {
                url: "vauchi://update".into(),
            };
        }

        // Capture display name during onboarding for identity persistence
        if self.screen == AppScreen::Onboarding
            && let UserAction::TextChanged {
                ref component_id,
                ref value,
            } = action
            && component_id == "display_name"
        {
            self.pending_display_name = Some(value.clone());
        }

        self.persist_settings_toggle(&action);

        if let Some(result) = self.intercept_exit_preview(&action) {
            return result;
        }

        if let Some(result) = self.intercept_add_field(&action) {
            return result;
        }

        if let Some(result) = self.intercept_settings_action(&action) {
            return result;
        }

        if let AppScreen::MyInfoEntryDetail { ref field_id } = self.screen {
            let field_id = field_id.clone();
            if let Some(result) = self.intercept_entry_detail_action(&field_id, &action) {
                return result;
            }
        }

        if let AppScreen::ContactDetail { ref contact_id } = self.screen {
            let contact_id = contact_id.clone();
            if let Some(result) = self.intercept_personal_note_change(&contact_id, &action) {
                return result;
            }
            if let Some(result) = self.intercept_field_note_change(&contact_id, &action) {
                return result;
            }
            if let Some(result) = self.intercept_proposal_trust_toggle(&contact_id, &action) {
                return result;
            }
            if let Some(result) = self.intercept_hide_toggle(&contact_id, &action) {
                return result;
            }
            if let Some(result) = self.intercept_contact_delete_archive(&contact_id, &action) {
                return result;
            }
        }

        // "Go exchange" from empty contacts or MyInfo → navigate to Exchange screen
        if matches!(
            &action,
            UserAction::ActionPressed { action_id } if action_id == "go_exchange"
        ) && matches!(self.screen, AppScreen::Contacts | AppScreen::MyInfo)
        {
            let screen = self.navigate_to(AppScreen::Exchange);
            return ActionResult::NavigateTo(screen);
        }

        if let Some(result) = self.handle_undo(&action) {
            return result;
        }

        let result = self.engine.handle_action(action);
        let result = self.route_result(result);
        let result = self.resolve_validation_error(result);
        self.apply_update_overlay_to_result(result)
    }
}

impl AppEngine {
    /// Poll the activity log and produce pending OS notifications.
    /// Public so PlatformAppEngine can expose it via UniFFI.
    pub fn poll_notifications(&mut self) -> Vec<PendingNotification> {
        self.drain_events_to_log();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Fetch raw rows from the activity log since the last poll.
        let rows = match self.vauchi.activity_log_poll(self.last_poll_time, now) {
            Ok(rows) => rows,
            Err(e) => {
                log::warn!("poll_notifications: activity_log_poll failed: {e}");
                return Vec::new();
            }
        };

        if rows.is_empty() {
            return Vec::new();
        }

        // Advance watermark based on rows *fetched*, not rows that survive
        // parsing/filtering. Unparsable or filtered rows must not cause
        // unbounded re-processing on every subsequent poll.
        self.last_poll_time = now;

        let entries: Vec<_> = rows
            .into_iter()
            .filter_map(|row| {
                let entry = serde_json::from_str::<ActivityLogEntry>(&row.payload).ok()?;
                Some((row.event_key, entry))
            })
            .collect();

        if entries.is_empty() {
            return Vec::new();
        }

        let prefs = NotificationPreferences {
            contact_added_enabled: self.vauchi.config().contact_added_notifications,
        };

        // Resolve contact names for body text
        let name_resolver = |contact_id: &str| {
            self.vauchi
                .storage()
                .load_contact(contact_id)
                .ok()
                .flatten()
                .map(|c| c.display_name().to_string())
                .unwrap_or_else(|| "Unknown contact".to_string())
        };

        NotificationEmitter::evaluate(&entries, &prefs, name_resolver)
    }

    /// Convert `ValidationError` into `UpdateScreen` with the error injected
    /// into the matching component. This ensures frontends never receive
    /// `ValidationError` and never need to patch the `ScreenModel` themselves.
    fn resolve_validation_error(&self, result: ActionResult) -> ActionResult {
        match result {
            ActionResult::ValidationError {
                component_id,
                message,
            } => {
                let screen = self
                    .engine
                    .current_screen()
                    .with_validation_error(&component_id, message);
                ActionResult::UpdateScreen(screen)
            }
            other => other,
        }
    }

    /// Apply the update overlay to any `ScreenModel` inside an `ActionResult`.
    fn apply_update_overlay_to_result(&self, result: ActionResult) -> ActionResult {
        match result {
            ActionResult::UpdateScreen(screen) => {
                ActionResult::UpdateScreen(self.apply_update_overlay(screen))
            }
            ActionResult::NavigateTo(screen) => {
                ActionResult::NavigateTo(self.apply_update_overlay(screen))
            }
            other => other,
        }
    }

    /// Drain the event receiver and write all events to the activity log.
    ///
    /// Returns newly inserted `(event_key, ActivityLogEntry)` pairs.
    /// Called before operations that read from the activity log (notifications)
    /// or when data mutations may have occurred (user actions).
    fn drain_events_to_log(&mut self) -> Vec<(String, ActivityLogEntry)> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }

        if events.is_empty() {
            return Vec::new();
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        match ActivityLogWriter::write(self.vauchi.storage(), &events, now) {
            Ok(entries) => entries,
            Err(e) => {
                log::error!("drain_events_to_log: ActivityLogWriter::write failed: {e}");
                Vec::new()
            }
        }
    }
}
