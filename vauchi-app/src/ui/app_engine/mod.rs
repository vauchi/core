// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Top-level application orchestrator.
//!
//! `AppEngine` wraps `Vauchi`, owns the active workflow engine,
//! handles navigation routing, and implements `WorkflowEngine` so
//! frontends see a single uniform interface.

mod ble_handshake;
mod device_link;
#[cfg(all(feature = "network-http", feature = "storage"))]
mod device_link_initiator;
mod intercept;
mod link_exchange;
mod link_responder;
mod multi_stage_exchange;
mod navigation;
mod routing;
mod screens;

pub use navigation::TabLayout;

use std::collections::HashMap;
use std::sync::mpsc;

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
/// Reserved global-chrome action id: the native top-bar gear forwards
/// this instead of constructing the "Settings" screen name. Resolved
/// to `NavigateTo(Settings)` before per-screen dispatch (CoreScreenIdMap
/// rework Tier-0; ADR-043 Amendment 4 — forward nav is core-resolved).
const ACTION_OPEN_SETTINGS: &str = "open_settings";
/// Action id used by the offline `Component::Banner` injected by
/// `apply_offline_overlay`. Currently presentational only — no
/// dispatcher arm. Frontends rendering the banner can ignore taps.
const ACTION_OFFLINE_BANNER: &str = "offline_banner";

/// Reserved action id for the demo-contact banner's dismiss button.
/// Emitted on `Component::Banner` from `apply_demo_contact_overlay`;
/// `handle_action` intercepts presses to call
/// `Vauchi::dismiss_demo_contact`. Per ADR-043 / ADR-021: the
/// state→banner mapping lives in core, not in any frontend's view
/// (was iOS `DemoContactCard` rendering a frontend-derived banner
/// from `viewModel.demoContact`).
const ACTION_DISMISS_DEMO_CONTACT: &str = "dismiss_demo_contact";

/// Action id for the sync-chrome `Component::Indicator` tap target.
/// Emitted on top-level screens by `apply_sync_chrome_overlay` when
/// the indicator is tappable (Idle or after a Failed attempt).
/// `handle_action` intercepts presses to call `Vauchi::sync`. Replaces
/// iOS's `HomeView.SyncStatusIndicator` (state→icon switch + 4
/// hardcoded English a11y strings — G1 of
/// `2026-05-02-ios-humble-ui-deep-retirement`).
const ACTION_SYNC_NOW: &str = "sync_now";

/// Last sync attempt result tracked by `AppEngine` and surfaced as
/// `Component::Indicator` chrome on every top-level screen via
/// `apply_sync_chrome_overlay`. Design: see
/// `_private/docs/designs/2026-05-28-sync-chrome-overlay-design.md`.
///
/// State source is the engine's own bookkeeping (set after each
/// `Vauchi::sync()` call from the `sync_now` handler), not the
/// `SyncController::connection_state()` — the design doc walks
/// through why: `Vauchi` does not field a `SyncController` today,
/// and the user-facing "Synced 15:47" / "Sync failed" model maps
/// cleanly onto the existing `VauchiSyncOutcome` return value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncChromeStatus {
    /// No sync attempt has been made in this session.
    Idle,
    /// Most recent sync succeeded — `unix_ts` is the wall-clock
    /// completion time (`self.clock.now()` at the time of success).
    Synced {
        /// Unix timestamp in seconds.
        unix_ts: u64,
    },
    /// Most recent sync attempt failed. The chrome chip surfaces a
    /// `Component::Indicator` with kind `Error` and a `sync_now`
    /// tap to retry.
    Failed,
}

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
    /// Change-app-password form (Settings → Change Password).
    ChangePassword,
    DecoyContacts,
    EmergencyShred,
    EmergencyBroadcast,
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
    /// Multi-stage face-to-face exchange.
    ///
    /// Renders the simultaneous bilateral QR + camera flow that
    /// `MobileMultiStageSession` drives via cycle-thread callbacks. The
    /// screen state mirrors `vauchi_core::exchange::ProtocolState`; the
    /// AppEngine bridge converts session listener callbacks into engine
    /// state mutations (see `multi_stage_exchange.rs` for the contract).
    ///
    /// `mode` carries the user's exchange-mode selection so the screen
    /// factory picks the right `MultiStageExchangeEngine` constructor
    /// (`new_hover` for `ExchangeMode::Hover`, `new_glance` for
    /// `ExchangeMode::Glance`). Pair 4 of
    /// `2026-04-28-pure-humble-ui-retire-native-screens` introduced
    /// the screen for Glance; Phase 1.E of
    /// `2026-05-11-hover-graduation-plan.md` added Hover and made the
    /// constructor mode-aware. Other modes (`Magic`, `Bump`, `Shake`,
    /// `Broadcast`, `TapHoverShake`, `Link`) continue to use the
    /// legacy `ExchangeStep::Qr`/`Ble`/`Link` sub-flows until their
    /// per-mode graduations land.
    MultiStageExchange {
        mode: vauchi_core::exchange::mode::ExchangeMode,
    },
    /// Link-mode **initiator** flow — generate a share URL, wait for the
    /// responder to open it, then retrieve + persist their card. The
    /// engine-owned `LinkInitiatorSession` (built on screen entry) drives
    /// the relay-escrow two-gate handshake; the `LinkExchangeEngine`
    /// renders the share-url / waiting / retrieving / terminal screens.
    /// Replaces the retired `ExchangeStep::Link` sub-flow (slice 32l
    /// Phase 3). Per
    /// `_private/docs/problems/2026-05-11-link-exchange-engine-graduation`.
    LinkExchange,
}

impl AppScreen {
    /// Whether this screen is a navigation root — a top-level entry
    /// point where pressing back should exit the app rather than pop
    /// `nav_history`. Drives `AppEngine::can_go_back`.
    ///
    /// The set is `Onboarding` plus the five mobile bottom-nav tabs
    /// (`MyInfo`, `Contacts`, `Exchange`, `Groups`, `More`). Onboarding
    /// is a root because it's the fresh-install entry; the five tab
    /// screens are roots because the bottom-nav lands there directly
    /// and the Android Material norm is that tab roots are back-stoppers.
    /// `Settings` is **not** a root: it is reached via the top-bar gear,
    /// which pushes onto history, so back must return to the prior screen.
    pub fn is_root(&self) -> bool {
        matches!(
            self,
            Self::Onboarding
                | Self::MyInfo
                | Self::Contacts
                | Self::Exchange
                | Self::Groups
                | Self::More
        )
    }

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
            Self::ChangePassword => "change_password",
            Self::DecoyContacts => "decoy_contacts",
            Self::EmergencyShred => "emergency_shred",
            Self::EmergencyBroadcast => "emergency_broadcast",
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
            Self::MultiStageExchange { .. } => "multi_stage_exchange",
            Self::LinkExchange => "link_exchange",
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
            "change_password" => Self::ChangePassword,
            "decoy_contacts" => Self::DecoyContacts,
            "emergency_shred" => Self::EmergencyShred,
            "emergency_broadcast" => Self::EmergencyBroadcast,
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
            // MultiStageExchange is parameterized ({ mode }) so it cannot
            // be constructed from the screen-id alone — falls through to
            // None like other parameterized screens (ContactDetail, etc.).
            _ => return None,
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
#[derive(Clone, Debug)]
pub(super) enum PendingContactUndo {
    Archive,
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
    /// Frontend-pushed render context — Category-1 settings per ADR-047
    /// (locale + theme_id). Owned by the frontend's OS-native sandbox;
    /// pushed via `set_render_context_json` at boot and on Settings
    /// dropdown changes. S2 of
    /// `_private/docs/planning/todo/2026-05-16-settings-storage-by-sensitivity-plan.md`.
    render_context: crate::ui::RenderContext,
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
    /// Last sync attempt result. Drives the `Component::Indicator`
    /// chrome chip injected on every top-level screen by
    /// `apply_sync_chrome_overlay`. Updated in the `sync_now` handler
    /// arm after each `Vauchi::sync()` call. Defaults to
    /// `SyncChromeStatus::Idle` on engine boot.
    sync_chrome_status: SyncChromeStatus,
    /// Screen-presentation [`Command`]s accumulated from
    /// `WorkflowEngine::screen_entered` / `screen_exited` callbacks
    /// during navigation. Frontends drain via
    /// [`Self::drain_pending_commands`] after each `navigate_to` /
    /// `navigate_back` / `handle_action` to apply hardware-side state
    /// (brightness, idle timer, future orientation lock, haptics).
    /// Phase 2b of `2026-05-04-exchange-command-screen-presentation`.
    pending_commands: std::collections::VecDeque<vauchi_core::Command>,
    /// Engine-owned link-mode responder machine (slice 32l Phase 2), live
    /// only on `AppScreen::DeepLinkResponder`. See `app_engine/link_responder.rs`.
    link_responder: Option<vauchi_core::exchange::link_responder::LinkResponderSession>,
    /// Engine-owned link-mode **initiator** machine (slice 32l Phase 3), live
    /// only on `AppScreen::LinkExchange`. See `app_engine/link_exchange.rs`.
    link_initiator: Option<vauchi_core::exchange::link_initiator::LinkInitiatorSession>,
    /// Engine-owned device-link **initiator** machine (slice 32l T3.1b), live
    /// only on `AppScreen::DeviceLinking`. See `app_engine/device_link_initiator.rs`.
    #[cfg(all(feature = "network-http", feature = "storage"))]
    device_link_initiator: Option<device_link_initiator::DeviceLinkInitiatorHolder>,
    /// Engine-owned multi-stage exchange machine (slice 32m T1.2b), live
    /// only on `AppScreen::MultiStageExchange`. See
    /// `app_engine/multi_stage_exchange.rs`. Replaces the
    /// `vauchi-platform::MobileMultiStageSession` cycle thread; T1.2c
    /// removes the parallel cycle-thread bridge in PlatformAppEngine
    /// to avoid double-driving the active engine on mobile.
    multi_stage_session: Option<multi_stage_exchange::MultiStageHolder>,
    /// Engine-owned BLE handshake machine (slice 32m T2.2b). Built
    /// on `Event::BleConnected` (or PlatformAppEngine on BLE-eligible
    /// screen entry), torn down by `cancel_ble_handshake_session`.
    /// Replaces the `MobileBleExchangeSession` cycle thread; T2.2c
    /// routes BLE events through `forward_ble_hardware_event`.
    ble_handshake_session: Option<ble_handshake::BleHandshakeHolder>,
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

    /// Returns the active render context (locale + theme_id) pushed
    /// by the frontend. Used by `screens.rs` to render Settings
    /// dropdown `selected` values without pulling state from the
    /// vault. See [ADR-047](../../../../_private/docs/decisions/adr-047-settings-storage-by-sensitivity.md).
    pub fn render_context(&self) -> &crate::ui::RenderContext {
        &self.render_context
    }

    /// Replace the active render context. Called from PAE
    /// `set_render_context_json` after JSON deserialization;
    /// frontends invoke this at boot and on every Settings
    /// locale/theme dropdown change. Invalidates the Settings
    /// screen cache AND — when the active screen is Settings —
    /// rebuilds the active engine so the next `current_screen()`
    /// call reflects the new dropdown `selected` value without
    /// requiring a navigate-away-and-back round trip.
    pub fn set_render_context(&mut self, ctx: crate::ui::RenderContext) {
        self.render_context = ctx;
        self.engine_cache.remove(&AppScreen::Settings);
        if matches!(self.screen, AppScreen::Settings) {
            let screen = self.screen.clone();
            self.engine = Self::create_engine(
                &self.vauchi,
                &screen,
                self.preview_as_contact.as_deref(),
                &self.device_capabilities,
                &self.render_context,
            );
        }
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
        let initial_render_context = crate::ui::RenderContext::default();
        let engine = Self::create_engine(&vauchi, &screen, None, &caps, &initial_render_context);
        let registry = vauchi_core::social::SocialNetworkRegistry::with_defaults();
        let field_catalog = vauchi_core::contact_card::FieldTypeCatalog::new(&registry);

        let now = vauchi.clock().unix_seconds();

        // Register a permanent event handler that sends VauchiEvents to a channel
        // for deferred persistence to the activity log (ADR-031).
        // Since VauchiEvent handler must be Sync but mpsc::Sender is not Sync,
        // we wrap it in a Mutex.
        let (event_tx, event_rx) = mpsc::channel();
        let event_tx = std::sync::Mutex::new(event_tx);
        let event_handler_id = vauchi.add_event_handler(std::sync::Arc::new(move |event| {
            if let Ok(tx) = event_tx.lock() {
                // best-effort: channel send fails only if the receiver
                // was dropped (AppEngine is being torn down)
                #[allow(clippy::let_underscore_must_use)]
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
            render_context: crate::ui::RenderContext::default(),
            update_status: AppUpdateStatus::UpToDate,
            update_dismissed: false,
            last_poll_time: now,
            event_rx,
            _event_handler_id: event_handler_id,
            pending_merge: None,
            pending_backup_reminder,
            network_online: true,
            sync_chrome_status: SyncChromeStatus::Idle,
            pending_commands: std::collections::VecDeque::new(),
            link_responder: None,
            link_initiator: None,
            #[cfg(all(feature = "network-http", feature = "storage"))]
            device_link_initiator: None,
            multi_stage_session: None,
            ble_handshake_session: None,
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

    /// Append commands to the pending queue. Phase 1.C.3e-v of the
    /// Hover graduation plan — `PlatformAppEngine`'s audio-listener
    /// bridge calls this to forward commands generated by the
    /// session's audio handshake (`Command::AudioEmitChallenge` /
    /// `AudioListenForResponse`) into the unified pending stream so
    /// the next `screen_envelope_to_json` drain surfaces them to the
    /// frontend.
    ///
    /// Frontends never call this directly — sessions emit via the
    /// listener path, engines emit via `ActionResult::Commands`, and
    /// the unified queue ensures both classes flow through the same
    /// drain.
    pub fn extend_pending_commands(
        &mut self,
        cmds: impl IntoIterator<Item = vauchi_core::Command>,
    ) {
        self.pending_commands.extend(cmds);
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
                // best-effort: reminder-shown bookkeeping; failure here
                // means the user may see the reminder again sooner
                #[allow(clippy::let_underscore_must_use)]
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
    pub fn set_version_policy(&mut self, policy: &VersionPolicy) {
        self.update_status =
            policy.evaluate(APP_COMPAT_VERSION, self.vauchi.clock().unix_seconds());
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

    /// Bridge from the multi-stage cycle thread — push an
    /// audio-proximity state transition from the platform-side
    /// orchestrator into the active `MultiStageExchangeEngine`'s
    /// view-state. Phase 1.C.3d of
    /// `_private/docs/planning/todo/2026-05-11-hover-graduation-plan.md`
    /// — sibling of `apply_multi_stage_state` mirroring the existing
    /// bridge pattern.
    ///
    /// Returns `true` if the active engine is the multi-stage one
    /// and the state was applied; `false` otherwise (caller is the
    /// audio-listener bridge in vauchi-platform; a `false` return
    /// indicates the user navigated away mid-handshake, which the
    /// bridge handles by dropping the callback).
    pub fn apply_multi_stage_audio_proximity(
        &mut self,
        state: vauchi_core::exchange::AudioProximityState,
    ) -> bool {
        if let Some(any) = self.engine.as_any_mut()
            && let Some(active) = any.downcast_mut::<crate::ui::MultiStageExchangeEngine>()
        {
            active.set_audio_proximity(state);
            return true;
        }
        false
    }

    /// `true` when the active engine is a `MultiStageExchangeEngine`
    /// constructed via [`MultiStageExchangeEngine::new_hover`].
    /// Phase 1.C polish — the platform-binding wire-up
    /// (`PlatformAppEngine::ensure_multi_stage_session`) reads this
    /// to decide whether to register the cycle-thread audio listener
    /// (see `try_autonomous_audio_trigger` mode gate). Returns
    /// `false` for Glance engines and for every non-multi-stage
    /// active engine. Until the Phase 1.E mode-dispatcher in
    /// `screens.rs` flips to per-mode constructors, this always
    /// returns `false`.
    pub fn is_active_engine_multi_stage_hover(&self) -> bool {
        if let Some(any) = self.engine.as_any()
            && let Some(active) = any.downcast_ref::<crate::ui::MultiStageExchangeEngine>()
        {
            return active.is_hover_mode();
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
        // Tier-0 (c) narrow collapse: these 5 families back engines that
        // emit per-sub-state `screen_id`s (`contact_list`, `backup_*`, …)
        // that `CoreScreenIdMap` was hand-folding. Stamp the canonical
        // `AppScreen::screen_id()` so frontends get a stable id. Narrow by
        // design — screens outside the set keep their engine id for in-flow
        // render-diffing and internal routing (the `backup_processing`
        // interception runs before this decorator). See the (c) plan.
        if matches!(
            self.screen,
            AppScreen::Contacts
                | AppScreen::Groups
                | AppScreen::DuressPin
                | AppScreen::Backup
                | AppScreen::Sync
        ) {
            screen.screen_id = self.screen.screen_id().to_string();
        }
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

    /// Inject a demo-contact banner on the Contacts screen when the
    /// onboarding demo is active. Scoped to `AppScreen::Contacts` so the
    /// banner doesn't leak onto other roots. Frontends render the
    /// generic `Component::Banner` and dispatch
    /// `ActionPressed { action_id: "dismiss_demo_contact" }` on tap of
    /// the action; `handle_action` calls `Vauchi::dismiss_demo_contact`.
    /// Idempotent — re-running doesn't duplicate the banner.
    ///
    /// Replaces iOS's `DemoContactCard` (~90 LOC) and the equivalent
    /// Android frontend rendering — both previously derived this from
    /// `viewModel.demoContact` frontend-side, which violated ADR-021's
    /// "core owns the state→presentation mapping" rule.
    fn apply_demo_contact_overlay(&self, mut screen: ScreenModel) -> ScreenModel {
        if !matches!(self.screen, AppScreen::Contacts) {
            return screen;
        }
        if !self.vauchi.is_demo_contact_active().unwrap_or(false) {
            return screen;
        }
        let card = match self.vauchi.demo_contact_card() {
            Ok(Some(card)) => card,
            _ => return screen,
        };
        let already_present = screen.components.iter().any(|c| {
            matches!(
                c,
                Component::Banner { action_id, .. } if action_id == ACTION_DISMISS_DEMO_CONTACT
            )
        });
        if already_present {
            return screen;
        }
        // Place at the top of the screen body so the onboarding hint
        // is the first thing visible above the contact list.
        screen.components.insert(
            0,
            Component::Banner {
                text: format!("{}: {}", card.tip_title, card.tip_content),
                action_label: "Dismiss".into(),
                action_id: ACTION_DISMISS_DEMO_CONTACT.into(),
                a11y: None,
            },
        );
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

    /// Inject the sync-chrome `Component::Indicator` on every emitted
    /// top-level screen. Idempotent. Skipped when offline (the
    /// `apply_offline_overlay` Banner already conveys "no network").
    /// Replaces iOS's `HomeView.SyncStatusIndicator` per G1 of
    /// `2026-05-02-ios-humble-ui-deep-retirement` — design at
    /// `_private/docs/designs/2026-05-28-sync-chrome-overlay-design.md`.
    ///
    /// State → presentation:
    /// - `Idle`: label "Sync", kind `Neutral`, action_id `Some("sync_now")`
    /// - `Synced { .. }`: label "Synced", kind `Active`, action_id `Some("sync_now")`
    /// - `Failed`: label "Sync failed", kind `Error`, action_id `Some("sync_now")`
    ///
    /// Timestamp formatting in the `Synced` label is deferred — a
    /// follow-up MR can render "Synced HH:MM" once locale-aware
    /// formatting is available on `AppEngine`.
    fn apply_sync_chrome_overlay(&self, mut screen: ScreenModel) -> ScreenModel {
        if !self.network_online {
            return screen;
        }
        let already_present = screen
            .components
            .iter()
            .any(|c| matches!(c, Component::Indicator { id, .. } if id == "sync"));
        if already_present {
            return screen;
        }
        let (label, kind) = match self.sync_chrome_status {
            SyncChromeStatus::Idle => {
                ("Sync".to_string(), super::component::IndicatorKind::Neutral)
            }
            SyncChromeStatus::Synced { .. } => (
                "Synced".to_string(),
                super::component::IndicatorKind::Active,
            ),
            SyncChromeStatus::Failed => (
                "Sync failed".to_string(),
                super::component::IndicatorKind::Error,
            ),
        };
        screen.components.insert(
            0,
            Component::Indicator {
                id: "sync".into(),
                label,
                kind,
                action_id: Some(ACTION_SYNC_NOW.into()),
                a11y: None,
            },
        );
        screen
    }
}

impl WorkflowEngine for AppEngine {
    fn current_screen(&self) -> ScreenModel {
        let screen = self.engine.current_screen();
        let screen = self.apply_update_overlay(screen);
        let screen = self.apply_offline_overlay(screen);
        let screen = self.apply_sync_chrome_overlay(screen);
        let screen = self.apply_demo_contact_overlay(screen);
        self.apply_screen_id_metadata(screen)
    }

    #[tracing::instrument(level = "debug", skip_all, name = "app.handle_action")]
    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        self.drain_events_to_log();

        // Handle sync_now from the chrome Indicator emitted by
        // apply_sync_chrome_overlay. Updates sync_chrome_status with
        // the outcome so the chip reflects the new state on next
        // render. No-op in builds without network-http feature.
        if matches!(
            &action,
            UserAction::ActionPressed { action_id } if action_id == ACTION_SYNC_NOW
        ) {
            #[cfg(feature = "network-http")]
            {
                use std::time::{SystemTime, UNIX_EPOCH};
                use vauchi_core::api::VauchiSyncOutcome;
                self.sync_chrome_status = match self.vauchi.sync() {
                    Ok(VauchiSyncOutcome::Ok { .. }) => SyncChromeStatus::Synced {
                        unix_ts: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0),
                    },
                    Ok(_) => self.sync_chrome_status,
                    Err(_) => SyncChromeStatus::Failed,
                };
            }
            return ActionResult::UpdateScreen(self.current_screen());
        }

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

        // Top-level / tab navigation (ADR-043 Amendment 4): a NavigateToTab
        // action carries the opaque token core minted on `TabInfo.action_id`.
        // Resolve it to a target screen and return `NavigateTo` *before*
        // per-screen dispatch, so the frontend never constructs a navigation
        // target. An unknown token (adversarial / stale) leaves the engine
        // where it is rather than navigating somewhere wrong — same
        // unknown-id stance as `route_result` (`from_screen_id` → `None`).
        if let UserAction::NavigateToTab { ref action_id } = action {
            return match AppScreen::from_screen_id(action_id) {
                Some(target) => ActionResult::NavigateTo(self.navigate_to(target)),
                None => ActionResult::UpdateScreen(self.engine.current_screen()),
            };
        }

        // Global-chrome navigation (ADR-043 Amendment 4): the native
        // top-bar gear forwards a reserved `ActionPressed` token rather
        // than constructing the Settings screen name. Resolve it to
        // `NavigateTo` before per-screen dispatch, on the same closed-set
        // basis as `open_update_link` above. Settings lives on its own
        // reserved id, so it cannot collide with the More-menu list item
        // (`"settings"`) or any per-screen `ActionPressed`.
        if matches!(
            action,
            UserAction::ActionPressed { ref action_id } if action_id == ACTION_OPEN_SETTINGS
        ) {
            return ActionResult::NavigateTo(self.navigate_to(AppScreen::Settings));
        }

        // Demo-contact dismiss (Tier-0 for the shell-purity spike): the
        // demo banner emitted by `apply_demo_contact_overlay` carries
        // this reserved id; intercept it here and call Vauchi to clear
        // the demo state, then re-render. Replaces iOS's
        // `viewModel.dismissDemoContact()` direct dispatch from
        // `DemoContactCard`.
        if matches!(
            action,
            UserAction::ActionPressed { ref action_id } if action_id == ACTION_DISMISS_DEMO_CONTACT
        ) {
            // Storage failure here is non-actionable from the action
            // loop; the next render still reflects the previous state, and
            // the user can retry. Log instead of panicking.
            if let Err(err) = self.vauchi.dismiss_demo_contact() {
                tracing::warn!(?err, "dismiss_demo_contact storage write failed");
            }
            return ActionResult::UpdateScreen(self.current_screen());
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
            // best-effort: engine recreate below reflects storage truth;
            // the canonical archive/unarchive path is `apply_contact_action`
            // which surfaces ShowAlert on failure
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.vauchi.unarchive_contact(contact_id);
            self.engine_cache.remove(&AppScreen::Contacts);
            self.engine_cache.remove(&AppScreen::ArchivedContacts);
            let screen = self.screen.clone();
            self.engine = Self::create_engine(
                &self.vauchi,
                &screen,
                self.preview_as_contact.as_deref(),
                &self.device_capabilities,
                &self.render_context,
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
        // Slice 32l T3.1b: feed typed DeviceLink* ActionResults into the engine-owned machine.
        #[cfg(all(feature = "network-http", feature = "storage"))]
        self.dispatch_device_link_side_effects(&result);
        let result = self.resolve_validation_error(result);
        self.apply_update_overlay_to_result(result)
    }
}

impl AppEngine {
    /// Poll the activity log and produce pending OS notifications.
    /// Public so PlatformAppEngine can expose it via UniFFI.
    pub fn poll_notifications(&mut self) -> Vec<PendingNotification> {
        self.drain_events_to_log();

        // Slice 32l T3.1b: advance the device-link machine one relay step (no-op when idle).
        #[cfg(all(feature = "network-http", feature = "storage"))]
        self.advance_device_link_session();
        // Slice 32m T1.2b: advance the multi-stage machine one
        // protocol step (no-op when idle / no active session).
        self.advance_multi_stage_session();

        let now = self.vauchi.clock().unix_seconds();

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

    /// Decorate a `NavigateTo`/`UpdateScreen` result's `ScreenModel` for
    /// the wire (overlays + `apply_screen_id_metadata`), mirroring
    /// `current_screen()` so results and re-reads agree. Runs after
    /// `route_result`, so internal sub-state `screen_id` routing is
    /// unaffected.
    fn apply_update_overlay_to_result(&self, result: ActionResult) -> ActionResult {
        match result {
            ActionResult::UpdateScreen(screen) => {
                ActionResult::UpdateScreen(self.apply_screen_id_metadata(
                    self.apply_offline_overlay(self.apply_update_overlay(screen)),
                ))
            }
            ActionResult::NavigateTo(screen) => {
                ActionResult::NavigateTo(self.apply_screen_id_metadata(
                    self.apply_offline_overlay(self.apply_update_overlay(screen)),
                ))
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

        let now = self.vauchi.clock().unix_seconds();

        match ActivityLogWriter::write(self.vauchi.storage(), &events, now) {
            Ok(entries) => entries,
            Err(e) => {
                log::error!("drain_events_to_log: ActivityLogWriter::write failed: {e}");
                Vec::new()
            }
        }
    }
}
