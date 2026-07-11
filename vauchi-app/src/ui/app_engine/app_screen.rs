// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `AppScreen` — the navigable screen space and its id/parent/
//! presentation metadata. Pure data + lookups; no `AppEngine` state.

use serde::{Deserialize, Serialize};

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
    /// Settings → Advanced sub-screen (M6 D6.1): network, delivery
    /// status, and emergency wipe, behind deliberate navigation.
    SettingsAdvanced,
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
    DeliveryStatus,
    Recovery,
    /// Helper-side recovery — vouch for a contact who lost their device.
    RecoveryHelp,
    Groups,
    /// Owner-private tag management list (ADR-051).
    Tags,
    /// Named-place management list (ADR-051).
    Places,
    /// Tag→group promotion review (ADR-051).
    TagPromotion {
        tag_id: String,
    },
    GroupDetail {
        group_id: String,
    },
    Privacy,
    Support,
    FormDialog {
        dialog_type: crate::ui::form_dialog::FormDialogType,
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
    /// Device-link join flow for a fresh device that has received a
    /// `DeviceLinkJoinInvitation` (QR scan, deep link, or pasted URL).
    /// Core parses the invitation, owns the responder machine, and
    /// navigates through name entry → confirmation code → adoption.
    DeviceLinkJoin {
        invitation_url: String,
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
    /// BLE in-person exchange (Magic/Bump/Shake) — the dedicated
    /// `BleExchangeEngine` drives discovery → handshake → exchange →
    /// proximity from a pure ScreenModel. `mode` selects the proximity
    /// signal layer. Replaces the legacy `ExchangeStep::Ble` sub-flow.
    /// Per `2026-05-11-ble-exchange-engine-graduation`.
    BleExchange {
        mode: vauchi_core::exchange::mode::ExchangeMode,
    },
    /// NFC in-person exchange (TapTap) — the dedicated `NfcExchangeEngine`
    /// drives the Send/Receive role choice → 3-phase tap handshake from a pure
    /// ScreenModel. Replaces the legacy `ExchangeStep::NfcRoleSelection` +
    /// `ExchangeStep::Nfc` sub-flow. Per the exchange-engine graduation program.
    NfcExchange,
    /// Cable (USB / direct-TCP) exchange — the dedicated `DirectTransportEngine`
    /// owns a `new_usb` `ExchangeSession` and drives the two-phase card-exchange
    /// ceremony from a pure ScreenModel. Replaces the legacy
    /// `ExchangeStep::DirectTransport` sub-flow. Per
    /// `2026-05-11-direct-transport-engine-graduation`.
    DirectTransport,
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
            Self::SettingsAdvanced => "settings_advanced",
            Self::Help => "help",
            Self::Backup => "backup",
            Self::Lock => "lock",
            Self::DeviceLinking => "device_linking",
            Self::DeviceManagement => "device_management",
            Self::DuressPin => "duress_pin",
            Self::ChangePassword => "change_password",
            Self::DecoyContacts => "decoy_contacts",
            Self::EmergencyShred => "emergency_shred",
            Self::DeliveryStatus => "delivery_status",
            Self::Recovery => "recovery",
            Self::RecoveryHelp => "recovery_help",
            Self::Groups => "groups",
            Self::Tags => "tags",
            Self::Places => "places",
            Self::TagPromotion { .. } => "tag_promotion",
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
            Self::DeviceLinkJoin { .. } => "device_link_join",
            Self::MultiStageExchange { .. } => "multi_stage_exchange",
            Self::LinkExchange => "link_exchange",
            Self::BleExchange { .. } => "ble_exchange",
            Self::NfcExchange => "nfc_exchange",
            Self::DirectTransport => "direct_transport",
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
            "delivery_status" => Self::DeliveryStatus,
            "recovery" => Self::Recovery,
            "recovery_help" => Self::RecoveryHelp,
            "groups" => Self::Groups,
            "tags" => Self::Tags,
            "places" => Self::Places,
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
            Self::Tags => Some("more"),
            Self::Places => Some("more"),
            Self::TagPromotion { .. } => Some("tags"),
            Self::RecoveryHelp | Self::RecoveryClaimReview => Some("recovery"),
            Self::DeviceLinking | Self::DeviceReplacement => Some("device_management"),
            Self::SettingsAdvanced => Some("settings"),
            _ => None,
        }
    }

    /// How the frontend should present this screen. Surfaced through
    /// `ScreenModel.presentation_kind` per
    /// `2026-05-01-screen-id-metadata-in-core` G2; replaces frontend-side
    /// substring checks (e.g. windows `screen_id == "form_dialog"`).
    pub fn presentation_kind(&self) -> crate::ui::screen::ScreenPresentationKind {
        use crate::ui::screen::ScreenPresentationKind;
        match self {
            Self::FormDialog { .. } => ScreenPresentationKind::Modal,
            _ => ScreenPresentationKind::Page,
        }
    }
}
