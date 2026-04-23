// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Navigation methods for `AppEngine` — screen stack management, back-navigation.

use super::AppEngine;
use super::AppScreen;
use crate::i18n::{Locale, get_string};
use crate::ui::screen::{ScreenModel, TabInfo};

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
        !matches!(screen, AppScreen::Lock | AppScreen::FormDialog { .. })
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
