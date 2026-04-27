// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Navigation methods for `AppEngine` — screen stack management, back-navigation.

use serde::{Deserialize, Serialize};

use super::AppEngine;
use super::AppScreen;
use crate::i18n::{Locale, get_string};
use crate::ui::screen::{ScreenModel, TabInfo};

/// Form-factor lens for tab-resolution queries.
///
/// Sub-screens collapse differently on mobile (5 bottom tabs, with
/// everything else routed to More) vs desktop (14 first-class sidebar
/// items including Settings, Recovery, Help, Backup, etc.). Frontends
/// pass the layout matching their nav surface; core returns the
/// canonical parent for that layout.
///
/// `Mobile` matches the set returned by `tab_info(locale)`;
/// `Desktop` matches the set returned by `sidebar_items(locale)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TabLayout {
    Mobile,
    Desktop,
}

impl AppScreen {
    /// Returns the parent tab this screen belongs to, given the
    /// frontend's tab layout. `None` for transient overlays
    /// (`Lock`, `FormDialog`) that do not select any tab.
    ///
    /// Replaces per-frontend `screenIdPrefixToAppScreen` /
    /// `MapScreenToParentId` tables (§1D pure-renderer remediation in
    /// `_private/docs/problems/2026-04-16-frontend-pure-renderer-violations`).
    /// Exhaustive — adding a new variant without a mapping is a
    /// compile error, which is the forcing function we want here.
    pub fn parent_tab_for(&self, layout: TabLayout) -> Option<AppScreen> {
        // First reduce parameterized variants to their non-parameter
        // canonical form so the layout match has fewer arms.
        let canonical = match self {
            Self::ContactDetail { .. }
            | Self::ContactEdit { .. }
            | Self::ContactVisibility { .. }
            | Self::VerifyFingerprint { .. }
            | Self::ContactMerge { .. } => Self::Contacts,
            Self::ContactDuplicates
            | Self::ContactLimit
            | Self::ArchivedContacts
            | Self::SocialGraph => Self::Contacts,
            Self::MyInfoEntryDetail { .. } | Self::AvatarEditor => Self::MyInfo,
            Self::GroupDetail { .. } => Self::Groups,
            Self::RecoveryHelp | Self::RecoveryClaimReview => Self::Recovery,
            Self::DeviceLinking | Self::DeviceReplacement => Self::DeviceManagement,
            Self::FormDialog { .. } => return None,
            Self::Lock => return None,
            // Deep-link consent is modal-shaped — no parent tab.
            Self::DeepLinkConsent { .. } => return None,
            other => other.clone(),
        };

        Some(match layout {
            TabLayout::Desktop => match canonical {
                // Top-level sidebar items (must mirror `sidebar_items`)
                Self::MyInfo
                | Self::Contacts
                | Self::Exchange
                | Self::Groups
                | Self::Settings
                | Self::Recovery
                | Self::DeviceManagement
                | Self::Backup
                | Self::Privacy
                | Self::Support
                | Self::Help
                | Self::ActivityLog
                | Self::Sync
                | Self::More
                | Self::Onboarding => canonical,
                // Settings sub-flows — collapse under Settings on
                // desktop (no More tab in the desktop sidebar).
                Self::DuressPin | Self::EmergencyShred => Self::Settings,
                // Exchange-side sync indicator.
                Self::DeliveryStatus => Self::Exchange,
                // Defensive default — should never trip thanks to the
                // canonical reduction above; kept so the match stays
                // total even if AppScreen grows.
                _ => return None,
            },
            TabLayout::Mobile => match canonical {
                // Top-level mobile tabs (must mirror
                // `available_screens` / `tab_info`).
                Self::MyInfo
                | Self::Contacts
                | Self::Exchange
                | Self::Groups
                | Self::More
                | Self::Onboarding => canonical,
                // Everything else lives under More on mobile.
                Self::Settings
                | Self::Recovery
                | Self::DeviceManagement
                | Self::Backup
                | Self::Privacy
                | Self::Support
                | Self::Help
                | Self::ActivityLog
                | Self::Sync
                | Self::DuressPin
                | Self::EmergencyShred => Self::More,
                Self::DeliveryStatus => Self::Exchange,
                _ => return None,
            },
        })
    }
}

impl AppEngine {
    pub fn navigate_to(&mut self, screen: AppScreen) -> ScreenModel {
        // Clear stale undo data — PII should not linger after user-initiated navigation
        self.pending_field_undo = None;
        self.pending_contact_undo = None;
        // Push the current screen to nav history before switching
        self.nav_history.push(self.screen.clone());
        self.navigate_to_internal(screen)
    }

    /// Navigate without pushing to history (used by back-navigation and completion routing).
    pub(super) fn navigate_to_internal(&mut self, screen: AppScreen) -> ScreenModel {
        // Refresh identity from storage if a sibling `Vauchi` instance
        // (e.g. `VauchiPlatform` on iOS/Android) wrote one to disk after
        // this AppEngine was constructed. Without this, screen builders
        // that read `vauchi.identity` directly (MyInfo, Contacts, etc.)
        // would still see `None` and render the unauthenticated/onboarding
        // variant — even after `has_identity()` correctly returned true
        // via the storage fallback. Idempotent.
        self.vauchi.refresh_identity_from_storage();

        // Swap in the new screen, get the old one back
        let old_screen = std::mem::replace(&mut self.screen, screen.clone());

        // Build or restore the engine for the new screen.
        // Pass preview_as_contact so MyInfo is built in PreviewAs mode when active.
        let preview_as = self.preview_as_contact.as_deref();
        let new_engine = self.engine_cache.remove(&screen).unwrap_or_else(|| {
            Self::create_engine(&self.vauchi, &screen, preview_as, &self.device_capabilities)
        });

        // Swap in the new engine, get the old one back
        let old_engine = std::mem::replace(&mut self.engine, new_engine);

        // Cache the old engine if its screen is cacheable
        if Self::is_cacheable(&old_screen) {
            self.engine_cache.insert(old_screen, old_engine);
        }

        self.engine.current_screen()
    }

    /// Navigate back using the history stack. Falls back to Home if empty.
    pub fn navigate_back(&mut self) -> ScreenModel {
        let target = self.nav_history.pop().unwrap_or(AppScreen::MyInfo);
        self.navigate_to_internal(target)
    }

    /// Screens that should never be cached — always start fresh.
    /// Onboarding IS cacheable: user navigates to FormDialog (add field)
    /// and back, must return to their current step with accumulated data.
    fn is_cacheable(screen: &AppScreen) -> bool {
        !matches!(
            screen,
            AppScreen::Lock | AppScreen::FormDialog { .. } | AppScreen::DeepLinkConsent { .. }
        )
    }

    /// Invalidates a cached engine for a specific screen.
    /// Next navigation to this screen will create a fresh engine.
    pub fn invalidate_screen(&mut self, screen: &AppScreen) {
        self.engine_cache.remove(screen);
    }

    /// Invalidates all cached engines. Use after bulk mutations.
    pub fn invalidate_all(&mut self) {
        self.engine_cache.clear();
    }

    /// Returns the default landing screen.
    /// Onboarding when no identity, Contacts when >=1 contact, MyInfo otherwise.
    pub fn default_screen(&self) -> AppScreen {
        if !self.vauchi.has_identity() {
            return AppScreen::Onboarding;
        }
        let has_contacts = self
            .vauchi
            .list_contacts()
            .map(|c| !c.is_empty())
            .unwrap_or(false);
        if has_contacts {
            AppScreen::Contacts
        } else {
            AppScreen::MyInfo
        }
    }

    /// Returns top-level navigation screens. Sub-screens (Sync,
    /// Recovery, Groups, Privacy, Support) are reached via `navigate_to`.
    pub fn available_screens(&self) -> Vec<AppScreen> {
        if !self.vauchi.has_identity() {
            return vec![AppScreen::Onboarding];
        }
        vec![
            AppScreen::MyInfo,
            AppScreen::Contacts,
            AppScreen::Exchange,
            AppScreen::Groups,
            AppScreen::More,
        ]
    }

    /// Returns metadata for the mobile bottom-tab bar — the 5 top-level
    /// tabs after identity creation (MyInfo, Contacts, Exchange, Groups,
    /// More); just Onboarding before. Labels resolve via
    /// `i18n::get_string(locale, "nav.*")` with English fallback.
    ///
    /// Frontends render the returned `TabInfo` directly — no local
    /// screen-to-tab map or label lookup needed.
    pub fn tab_info(&self, locale: Locale) -> Vec<TabInfo> {
        self.available_screens()
            .into_iter()
            .map(|screen| Self::tab_info_for(screen, locale))
            .collect()
    }

    /// Returns metadata for a desktop sidebar — all top-level navigable
    /// screens (MyInfo, Contacts, Exchange, Groups, Settings, Recovery,
    /// Device Management, Backup, Privacy, Support, Help, Activity Log,
    /// Sync, More). Wider than `tab_info()` because desktop frames have
    /// vertical space that a phone bottom-tab bar does not. Labels
    /// resolve via `i18n::get_string(locale, "nav.*")`.
    ///
    /// Used by linux-gtk, linux-qt, Windows, macOS sidebars so those
    /// frontends can stop maintaining their own `AppScreen`-to-label
    /// match tables (§6 pure-renderer remediation).
    pub fn sidebar_items(&self, locale: Locale) -> Vec<TabInfo> {
        if !self.vauchi.has_identity() {
            return vec![Self::tab_info_for(AppScreen::Onboarding, locale)];
        }
        [
            AppScreen::MyInfo,
            AppScreen::Contacts,
            AppScreen::Exchange,
            AppScreen::Groups,
            AppScreen::Settings,
            AppScreen::Recovery,
            AppScreen::DeviceManagement,
            AppScreen::Backup,
            AppScreen::Privacy,
            AppScreen::Support,
            AppScreen::Help,
            AppScreen::ActivityLog,
            AppScreen::Sync,
            AppScreen::More,
        ]
        .into_iter()
        .map(|s| Self::tab_info_for(s, locale))
        .collect()
    }

    /// Returns the canonical screen-id of the parent tab the active
    /// screen belongs to, or `None` for transient overlays (`Lock`,
    /// `FormDialog`). The id matches one of the `id` values returned
    /// by `tab_info` (mobile) / `sidebar_items` (desktop) for the
    /// same layout, so frontends can use it directly to drive sidebar
    /// or bottom-tab selection state.
    ///
    /// Replaces per-frontend `screenIdPrefixToAppScreen` /
    /// `MapScreenToParentId` tables (§1D pure-renderer remediation).
    pub fn current_tab_id(&self, layout: TabLayout) -> Option<&'static str> {
        self.screen.parent_tab_for(layout).map(|s| s.screen_id())
    }

    /// Resolve a `TabInfo` for a single `AppScreen`: looks up the
    /// localized label, returns the SF Symbol icon name, and produces
    /// a zero badge count (callers can overlay badge counts separately).
    fn tab_info_for(screen: AppScreen, locale: Locale) -> TabInfo {
        let (key, icon, fallback) = match &screen {
            AppScreen::MyInfo => ("nav.myCard", "person.crop.rectangle", "My Card"),
            AppScreen::Contacts => ("nav.contacts", "person.2", "Contacts"),
            AppScreen::Exchange => ("nav.exchange", "qrcode", "Exchange"),
            AppScreen::Groups => ("nav.groups", "folder", "Groups"),
            AppScreen::More => ("nav.more", "ellipsis.circle", "More"),
            AppScreen::Onboarding => ("nav.onboarding", "person.badge.plus", "Welcome"),
            AppScreen::Settings => ("nav.settings", "gearshape", "Settings"),
            AppScreen::Help => ("nav.help", "questionmark.circle", "Help"),
            AppScreen::Recovery => ("nav.recovery", "key.horizontal", "Recovery"),
            AppScreen::DeviceManagement => ("nav.devices", "laptopcomputer", "Devices"),
            AppScreen::Backup => ("nav.backup", "externaldrive", "Backup"),
            AppScreen::Privacy => ("nav.privacy", "hand.raised", "Privacy"),
            AppScreen::Support => ("nav.support", "bubble.left.and.bubble.right", "Support"),
            AppScreen::ActivityLog => ("nav.activity", "list.bullet.rectangle", "Activity"),
            AppScreen::Sync => ("nav.sync", "arrow.triangle.2.circlepath", "Sync"),
            _ => ("nav.home", "house", "Home"),
        };
        // `get_string` returns the sentinel "Missing: <key>" when the key
        // is absent from both the requested locale and the English
        // fallback; fall back to the hardcoded English label so UIs
        // never display the sentinel.
        let label = get_string(locale, key);
        let label = if label.starts_with("Missing:") {
            fallback.to_string()
        } else {
            label
        };
        TabInfo {
            id: screen.screen_id().to_string(),
            label,
            icon: icon.to_string(),
            badge_count: 0,
        }
    }
}
