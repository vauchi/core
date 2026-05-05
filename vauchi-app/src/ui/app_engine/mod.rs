// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Top-level application orchestrator.
//!
//! `AppEngine` wraps `Vauchi`, owns the active workflow engine,
//! handles navigation routing, and implements `WorkflowEngine` so
//! frontends see a single uniform interface.

mod device_link;
mod intercept;
mod navigation;
mod routing;
mod screens;

pub use navigation::TabLayout;

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
use super::engine::WorkflowEngine;
use super::screen::{ActionStyle, ScreenAction, ScreenModel};

/// Shared action ID for the update link button/banner.
const ACTION_OPEN_UPDATE_LINK: &str = "open_update_link";
/// Action id used by the offline `Component::Banner` injected by
/// `apply_offline_overlay`. Currently presentational only — no
/// dispatcher arm. Frontends rendering the banner can ignore taps.
const ACTION_OFFLINE_BANNER: &str = "offline_banner";

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
    DecoyContacts,
    EmergencyShred,
    DeliveryStatus,
    Sync,
    Recovery,
    /// Helper-side recovery — vouch for a contact who lost their device.
    RecoveryHelp,
    /// Social graph view — contacts grouped by trust level.
    SocialGraph,
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
    ArchivedContacts,
    DeviceReplacement,
    AvatarEditor,
    RecoveryClaimReview,
    /// Consent gate for an incoming `vauchi://exchange?...` deep link.
    /// Holds the parsed payload until the user grants or denies. Per
    /// `_private/docs/problems/2026-04-25-deeplink-consent-orchestrator`.
    DeepLinkConsent {
        payload: vauchi_core::exchange::link_mode::DeepLinkPayload,
    },
    /// Post-grant link-mode responder flow — drives the cycle thread
    /// through Polling / Retrieving until a contact is finalized.
    /// Per `_private/docs/problems/2026-04-27-deep-link-responder-flow`.
    DeepLinkResponder {
        payload: vauchi_core::exchange::link_mode::DeepLinkPayload,
    },
    /// Multi-stage face-to-face exchange (Pair 4 of pure-humble-ui-retire-native-screens).
    ///
    /// Renders the simultaneous bilateral QR + camera flow that
    /// `MobileMultiStageSession` drives via cycle-thread callbacks. The
    /// screen state mirrors `vauchi_core::exchange::ProtocolState`; the
    /// AppEngine bridge converts session listener callbacks into engine
    /// state mutations (see `multi_stage_exchange.rs` for the contract).
    MultiStageExchange,
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
            Self::DecoyContacts => "decoy_contacts",
            Self::EmergencyShred => "emergency_shred",
            Self::DeliveryStatus => "delivery_status",
            Self::Sync => "sync",
            Self::Recovery => "recovery",
            Self::RecoveryHelp => "recovery_help",
            Self::SocialGraph => "social_graph",
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
            Self::ArchivedContacts => "archived_contacts",
            Self::DeviceReplacement => "device_replacement",
            Self::AvatarEditor => "avatar_editor",
            Self::RecoveryClaimReview => "recovery_claim_review",
            Self::DeepLinkConsent { .. } => "deep_link_consent",
            Self::DeepLinkResponder { .. } => "deep_link_responder",
            Self::MultiStageExchange => "multi_stage_exchange",
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
            "device_management" => Self::DeviceManagement,
            "duress_pin" => Self::DuressPin,
            "decoy_contacts" => Self::DecoyContacts,
            "emergency_shred" => Self::EmergencyShred,
            "delivery_status" => Self::DeliveryStatus,
            "sync" => Self::Sync,
            "recovery" => Self::Recovery,
            "recovery_help" => Self::RecoveryHelp,
            "social_graph" => Self::SocialGraph,
            "groups" => Self::Groups,
            "privacy" => Self::Privacy,
            "support" => Self::Support,
            "contact_duplicates" => Self::ContactDuplicates,
            "contact_limit" => Self::ContactLimit,
            "more" => Self::More,
            "activity_log" => Self::ActivityLog,
            "archived_contacts" => Self::ArchivedContacts,
            "device_replacement" => Self::DeviceReplacement,
            "avatar_editor" => Self::AvatarEditor,
            "recovery_claim_review" => Self::RecoveryClaimReview,
            "multi_stage_exchange" => Self::MultiStageExchange,
            _ => return None,
        })
    }

    /// Parse a screen name + parameter into a parameterized `AppScreen`.
    ///
    /// Used by CABI frontends that receive `OpenContact { contact_id }` etc.
    /// and need to navigate to the target screen.
    pub fn from_screen_id_with_param(id: &str, param: &str) -> Option<Self> {
        let param = param.to_string();
        Some(match id {
            "contact_detail" => Self::ContactDetail { contact_id: param },
            "contact_edit" => Self::ContactEdit { contact_id: param },
            "contact_visibility" => Self::ContactVisibility { contact_id: param },
            "entry_detail" => Self::MyInfoEntryDetail { field_id: param },
            "group_detail" => Self::GroupDetail { group_id: param },
            "verify_fingerprint" => Self::VerifyFingerprint { contact_id: param },
            _ => return Self::from_screen_id(id),
        })
    }

    /// The sidebar/tab this screen belongs to, if it is a sub-screen of
    /// a top-level tab. `None` for top-level tabs themselves and for
    /// transient/global screens (lock, onboarding, deep-link consent).
    /// Surfaced through `ScreenModel.parent_screen_id` per
    /// `2026-05-01-screen-id-metadata-in-core` G1; replaces
    /// `MapScreenToParentId`-style switch statements in frontends.
    pub fn parent_screen_id(&self) -> Option<&'static str> {
        match self {
            Self::MyInfoEntryDetail { .. } => Some("my_info"),
            Self::ContactDetail { .. }
            | Self::ContactEdit { .. }
            | Self::ContactVisibility { .. }
            | Self::ContactDuplicates
            | Self::ContactMerge { .. }
            | Self::ContactLimit
            | Self::ArchivedContacts
            | Self::VerifyFingerprint { .. } => Some("contacts"),
            Self::GroupDetail { .. } => Some("groups"),
            Self::RecoveryHelp | Self::RecoveryClaimReview => Some("recovery"),
            Self::DeviceLinking | Self::DeviceReplacement => Some("device_management"),
            _ => None,
        }
    }

    /// How the frontend should present this screen. Surfaced through
    /// `ScreenModel.presentation_kind` per
    /// `2026-05-01-screen-id-metadata-in-core` G2; replaces frontend-side
    /// substring checks (e.g. windows `screen_id == "form_dialog"`).
    pub fn presentation_kind(&self) -> super::screen::ScreenPresentationKind {
        use super::screen::ScreenPresentationKind;
        match self {
            Self::FormDialog { .. } => ScreenPresentationKind::Modal,
            _ => ScreenPresentationKind::Page,
        }
    }
}

/// Tracks which contact undo is pending (archive only — delete is now irrevocable).
///
/// The `contact_id` field exists for debugging and potential future use
/// (e.g. confirming the undo matches the expected contact).
#[derive(Clone, Debug)]
pub(super) enum PendingContactUndo {
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
    /// Captured from backup TextChanged events for backup execution.
    pending_backup_password: Option<String>,
    /// Captured from backup ItemToggled events (default: Full).
    pending_backup_full: bool,
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
    /// IDs of the two contacts selected for merge (primary_id, secondary_id).
    /// Set when the user confirms a merge from DuplicateDetection; consumed by
    /// handle_completion for ContactMerge.
    pub(super) pending_merge: Option<(String, String)>,
    /// Whether a backup reminder toast should be shown on next main-screen render.
    pending_backup_reminder: bool,
    /// Frontend-reported network reachability (`NWPathMonitor` on iOS,
    /// `ConnectivityManager` on Android). When `false`, every emitted
    /// `ScreenModel` is decorated with an offline `Component::Banner`
    /// via `apply_offline_overlay`, so frontends never decide whether
    /// to render the banner themselves (audit
    /// `2026-04-28-lifecycle-session-residue-umbrella` item P2-D).
    /// Defaults to `true` so installs without a network monitor (CLI,
    /// tests, embedded) behave as if always-online.
    network_online: bool,
    /// Screen-presentation [`Command`]s accumulated from
    /// `WorkflowEngine::screen_entered` / `screen_exited` callbacks
    /// during navigation. Frontends drain via
    /// [`Self::drain_pending_commands`] after each `navigate_to` /
    /// `navigate_back` / `handle_action` to apply hardware-side state
    /// (brightness, idle timer, future orientation lock, haptics).
    /// Phase 2b of `2026-05-04-exchange-command-screen-presentation`.
    pending_commands: std::collections::VecDeque<vauchi_core::Command>,
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
        // Boot decision (audit
        // `2026-04-28-app-launch-and-identity-orchestration-in-core`
        // §2.1). Frontends call `boot()` and trust the returned
        // `ScreenModel` — they do not duplicate this decision tree.
        //
        // The "identity exists but onboarding never marked complete"
        // resume-path called out by §2.5 is gated behind a future
        // migration: existing installs ship `OnboardingProgress` at
        // default (`completed_at == None`) so honouring the flag
        // today would route every legacy user back through
        // onboarding. Until the legacy heal lands, "identity exists"
        // implies "past onboarding"; the atomic
        // `create_identity_with_onboarding` helper still closes the
        // *new* crash window for installs after this commit.
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

        // Check if a backup reminder is due (only if identity exists and not on lock/onboarding).
        let pending_backup_reminder = matches!(screen, AppScreen::MyInfo | AppScreen::Contacts)
            && vauchi
                .identity()
                .and_then(|id| {
                    let fallback = id.device_info().created_at();
                    vauchi
                        .load_backup_reminder_state()
                        .ok()
                        .map(|state| state.is_reminder_due(now, fallback))
                })
                .unwrap_or(false);

        Self {
            vauchi,
            screen,
            engine,
            engine_cache: HashMap::new(),
            pending_display_name: None,
            pending_backup_password: None,
            pending_backup_full: true,
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
            pending_merge: None,
            pending_backup_reminder,
            network_online: true,
            pending_commands: std::collections::VecDeque::new(),
        }
    }

    /// Drain and return all `Command`s accumulated from `WorkflowEngine`
    /// lifecycle hooks (`screen_entered` / `screen_exited`) during recent
    /// navigation. Frontends call this after every action / navigate to
    /// pick up brightness, idle-timer, and (Phase 2c) orientation-lock
    /// commands that core has emitted in response to screen transitions.
    pub fn drain_pending_commands(&mut self) -> Vec<vauchi_core::Command> {
        self.pending_commands.drain(..).collect()
    }

    /// Check and drain a pending backup reminder.
    ///
    /// Returns `Some(ShowToast { .. })` if a backup reminder is due, `None` otherwise.
    /// Frontends should call this after initialization or after unlocking.
    /// The toast `undo_action_id` is `"backup_now"` — pressing it navigates to Backup.
    pub fn drain_backup_reminder(&mut self) -> Option<ActionResult> {
        if self.pending_backup_reminder {
            self.pending_backup_reminder = false;
            // Record that we showed a reminder
            if let Ok(mut state) = self.vauchi.load_backup_reminder_state() {
                state.record_reminder_shown();
                let _ = self.vauchi.save_backup_reminder_state(&state);
            }
            Some(ActionResult::ShowToast {
                message: "You haven't backed up in a while. Back up now to protect your identity."
                    .into(),
                undo_action_id: Some("backup_now".into()),
            })
        } else {
            None
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

    /// Bridge from the multi-stage cycle thread — push a state
    /// transition into the active `MultiStageExchangeEngine`.
    ///
    /// No-op when the active engine is not the multi-stage one
    /// (frontend left the screen between callback dispatch and lock
    /// acquisition). Returns `true` when the bridge applied the
    /// state, `false` otherwise — useful for the platform layer to
    /// decide whether to fire screen-invalidation notifications.
    ///
    /// Pair 4 of `_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens`.
    pub fn apply_multi_stage_state(&mut self, state: vauchi_core::exchange::ProtocolState) -> bool {
        if let Some(any) = self.engine.as_any_mut()
            && let Some(active) = any.downcast_mut::<crate::ui::MultiStageExchangeEngine>()
        {
            active.set_state(state);
            return true;
        }
        false
    }

    /// Bridge from the multi-stage cycle thread — push the latest QR
    /// payload (own card) into the active `MultiStageExchangeEngine`.
    pub fn apply_multi_stage_qr_payload(
        &mut self,
        payload: &vauchi_core::exchange::QrPayload,
    ) -> bool {
        if let Some(any) = self.engine.as_any_mut()
            && let Some(active) = any.downcast_mut::<crate::ui::MultiStageExchangeEngine>()
        {
            active.set_qr_payload(payload);
            return true;
        }
        false
    }

    /// Bridge from the multi-stage cycle thread — record the peer
    /// display name on the `Finalized` transition.
    pub fn apply_multi_stage_finalized(&mut self, contact_name: String) -> bool {
        if let Some(any) = self.engine.as_any_mut()
            && let Some(active) = any.downcast_mut::<crate::ui::MultiStageExchangeEngine>()
        {
            active.set_finalized(contact_name);
            return true;
        }
        false
    }

    /// Bridge from the multi-stage cycle thread — flag the cycle as
    /// ended so the engine flips to the success / failure terminal
    /// chrome.
    pub fn apply_multi_stage_session_ended(&mut self) -> bool {
        if let Some(any) = self.engine.as_any_mut()
            && let Some(active) = any.downcast_mut::<crate::ui::MultiStageExchangeEngine>()
        {
            active.set_session_ended();
            return true;
        }
        false
    }

    /// Set the frontend-reported network reachability.
    ///
    /// Frontends call this from their `NWPathMonitor` (iOS) or
    /// `ConnectivityManager` (Android) callback. The decision of
    /// "is this network usable for sync" stays in core; the
    /// frontend just forwards the platform signal. While
    /// `network_online == false`, every emitted `ScreenModel` is
    /// decorated with an offline `Component::Banner` via
    /// `apply_offline_overlay`. Audit
    /// `2026-04-28-lifecycle-session-residue-umbrella` P2-D.
    pub fn set_network_online(&mut self, online: bool) {
        self.network_online = online;
    }

    /// Returns the last frontend-reported network reachability.
    pub fn is_network_online(&self) -> bool {
        self.network_online
    }

    /// Decorate the given screen with an offline `Component::Banner`
    /// Stamp `parent_screen_id` and `presentation_kind` onto the
    /// inner-engine's screen model from the AppEngine-level
    /// `AppScreen`. Per `2026-05-01-screen-id-metadata-in-core` G1+G2 —
    /// frontends consume these instead of substring-matching
    /// `screen_id` strings.
    ///
    /// Idempotent: if the inner engine already set non-default values
    /// (it shouldn't, but the contract is clear), they are preserved.
    fn apply_screen_id_metadata(&self, mut screen: ScreenModel) -> ScreenModel {
        if screen.parent_screen_id.is_none() {
            screen.parent_screen_id = self.screen.parent_screen_id().map(String::from);
        }
        if matches!(
            screen.presentation_kind,
            crate::ui::screen::ScreenPresentationKind::Page
        ) {
            screen.presentation_kind = self.screen.presentation_kind();
        }
        screen
    }

    /// when `network_online == false`. Idempotent — only inserts a
    /// banner; never duplicates one already present.
    ///
    /// Inserted at the bottom of the existing components so an
    /// active update banner (`apply_update_overlay`) keeps its
    /// top-of-screen position.
    fn apply_offline_overlay(&self, mut screen: ScreenModel) -> ScreenModel {
        if self.network_online {
            return screen;
        }
        let already_present = screen.components.iter().any(|c| {
            matches!(
                c,
                Component::Banner { action_id, .. } if action_id == ACTION_OFFLINE_BANNER
            )
        });
        if already_present {
            return screen;
        }
        screen.components.push(Component::Banner {
            text: "You're offline. Changes will sync when you reconnect.".into(),
            action_label: String::new(),
            action_id: ACTION_OFFLINE_BANNER.into(),
            a11y: None,
        });
        screen
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
                        a11y: None,
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
                        a11y: None,
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
                    a11y: None,
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
    use super::{AppScreen, initials};

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

    // @internal
    #[test]
    fn from_screen_id_with_param_contact_detail() {
        let screen = AppScreen::from_screen_id_with_param("contact_detail", "abc-123");
        assert_eq!(
            screen,
            Some(AppScreen::ContactDetail {
                contact_id: "abc-123".to_string()
            })
        );
    }

    // @internal
    #[test]
    fn from_screen_id_with_param_falls_back() {
        let screen = AppScreen::from_screen_id_with_param("contacts", "ignored");
        assert_eq!(screen, Some(AppScreen::Contacts));
    }

    // @internal
    #[test]
    fn from_screen_id_with_param_unknown() {
        let screen = AppScreen::from_screen_id_with_param("nonexistent", "x");
        assert_eq!(screen, None);
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
        let screen = self.apply_update_overlay(screen);
        let screen = self.apply_offline_overlay(screen);
        self.apply_screen_id_metadata(screen)
    }

    #[tracing::instrument(level = "debug", skip_all, name = "app.handle_action")]
    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        self.drain_events_to_log();

        // Handle backup reminder toast action
        if matches!(
            &action,
            UserAction::ActionPressed { action_id } if action_id == "backup_now"
        ) {
            return ActionResult::NavigateTo(self.navigate_to(AppScreen::Backup));
        }

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

        // Capture backup password and level toggle during backup flow
        if self.screen == AppScreen::Backup {
            match &action {
                UserAction::TextChanged {
                    component_id,
                    value,
                } if component_id == "password" => {
                    self.pending_backup_password = Some(value.clone());
                }
                UserAction::ItemToggled {
                    component_id,
                    item_id,
                } if component_id == "backup_level" && item_id == "level_toggle" => {
                    self.pending_backup_full = !self.pending_backup_full;
                }
                _ => {}
            }
        }

        self.persist_settings_toggle(&action);

        if let Some(result) = self.intercept_exit_preview(&action) {
            return result;
        }

        if let Some(result) = self.intercept_edit_avatar(&action) {
            return result;
        }

        if let Some(result) = self.intercept_add_field(&action) {
            return result;
        }

        if let Some(result) = self.intercept_settings_action(&action) {
            return result;
        }

        if let Some(result) = self.intercept_decoy_contacts_action(&action) {
            return result;
        }

        if let Some(result) = self.intercept_info_requested(&action) {
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
            if let Some(result) = self.intercept_recovery_trust_toggle(&contact_id, &action) {
                return result;
            }
            if let Some(result) = self.intercept_hide_toggle(&contact_id, &action) {
                return result;
            }
            if let Some(result) = self.intercept_contact_delete_archive(&contact_id, &action) {
                return result;
            }
        }

        // "Go exchange" (empty-state CTA) and "Add contact" (always-visible
        // Primary button on the contact list) share the same target — on
        // MVP we only acquire contacts via in-person exchange, so both
        // affordances carry the same user intent. `add_contact` is only
        // emitted on Contacts; `go_exchange` also fires from MyInfo when
        // the list is empty. A dedicated VCF-import path can later redirect
        // `add_contact` via a frontend capability hint.
        if matches!(
            &action,
            UserAction::ActionPressed { action_id } if action_id == "go_exchange" || action_id == "add_contact"
        ) && matches!(self.screen, AppScreen::Contacts | AppScreen::MyInfo)
        {
            let screen = self.navigate_to(AppScreen::Exchange);
            return ActionResult::NavigateTo(screen);
        }

        // "View archived" from contacts → navigate to ArchivedContacts screen
        if matches!(
            &action,
            UserAction::ActionPressed { action_id } if action_id == "view_archived"
        ) && matches!(self.screen, AppScreen::Contacts)
        {
            let screen = self.navigate_to(AppScreen::ArchivedContacts);
            return ActionResult::NavigateTo(screen);
        }

        // "Find duplicates" from contacts → navigate to ContactDuplicates screen
        if matches!(
            &action,
            UserAction::ActionPressed { action_id } if action_id == "find_duplicates"
        ) && matches!(self.screen, AppScreen::Contacts)
        {
            let screen = self.navigate_to(AppScreen::ContactDuplicates);
            return ActionResult::NavigateTo(screen);
        }

        // "merge" from ContactDuplicates → store pending pair and navigate to ContactMerge
        if self.screen == AppScreen::ContactDuplicates
            && matches!(&action, UserAction::ActionPressed { action_id } if action_id == "merge")
            && let Some(result) = self.intercept_merge_action()
        {
            return result;
        }

        // "dismiss" from ContactDuplicates → drop the selected pair from
        // the duplicate set (reversible — re-detects on next find_duplicates).
        if self.screen == AppScreen::ContactDuplicates
            && matches!(&action, UserAction::ActionPressed { action_id } if action_id == "dismiss")
            && let Some(result) = self.intercept_dismiss_duplicate_action()
        {
            return result;
        }

        // RecoveryHelp screen: parse claim + create voucher need Vauchi
        // access (identity keypair for signing) so they're handled at the
        // AppEngine layer rather than inside the engine.
        if self.screen == AppScreen::RecoveryHelp
            && matches!(&action, UserAction::ActionPressed { action_id } if action_id == "verify_claim")
            && let Some(result) = self.intercept_verify_claim_action()
        {
            return result;
        }
        if self.screen == AppScreen::RecoveryHelp
            && matches!(&action, UserAction::ActionPressed { action_id } if action_id == "create_voucher")
            && let Some(result) = self.intercept_create_voucher_action()
        {
            return result;
        }

        // Recovery screen (EnterOldKey step): hex-decode + sign claim
        // need Vauchi/Identity access; engine signals Complete and the
        // intercept does the actual work via Vauchi::create_recovery_claim_hex_b64.
        if self.screen == AppScreen::Recovery
            && matches!(&action, UserAction::ActionPressed { action_id } if action_id == "create_claim")
            && let Some(result) = self.intercept_create_claim_action()
        {
            return result;
        }

        // Unarchive from ArchivedContacts screen
        if self.screen == AppScreen::ArchivedContacts
            && let UserAction::ActionPressed { ref action_id } = action
            && let Some(contact_id) = action_id.strip_prefix("unarchive_")
        {
            let _ = self.vauchi.unarchive_contact(contact_id);
            self.engine_cache.remove(&AppScreen::Contacts);
            self.engine_cache.remove(&AppScreen::ArchivedContacts);
            let screen = self.screen.clone();
            self.engine = Self::create_engine(
                &self.vauchi,
                &screen,
                self.preview_as_contact.as_deref(),
                &self.device_capabilities,
            );
            return ActionResult::ShowToast {
                message: "Contact unarchived".into(),
                undo_action_id: None,
            };
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
            ActionResult::UpdateScreen(screen) => ActionResult::UpdateScreen(
                self.apply_offline_overlay(self.apply_update_overlay(screen)),
            ),
            ActionResult::NavigateTo(screen) => ActionResult::NavigateTo(
                self.apply_offline_overlay(self.apply_update_overlay(screen)),
            ),
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
