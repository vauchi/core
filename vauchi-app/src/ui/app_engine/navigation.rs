// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Navigation methods for `AppEngine` — screen stack management, back-navigation.

use super::AppEngine;
use super::AppScreen;
use crate::ui::screen::ScreenModel;

impl AppEngine {
    pub fn navigate_to(&mut self, screen: AppScreen) -> ScreenModel {
        // Clear stale undo data — field PII should not linger after user-initiated navigation
        self.pending_field_undo = None;
        // Push the current screen to nav history before switching
        self.nav_history.push(self.screen.clone());
        self.navigate_to_internal(screen)
    }

    /// Navigate without pushing to history (used by back-navigation and completion routing).
    pub(super) fn navigate_to_internal(&mut self, screen: AppScreen) -> ScreenModel {
        // Swap in the new screen, get the old one back
        let old_screen = std::mem::replace(&mut self.screen, screen.clone());

        // Build or restore the engine for the new screen
        let new_engine = self
            .engine_cache
            .remove(&screen)
            .unwrap_or_else(|| Self::create_engine(&self.vauchi, &screen));

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

    /// Returns top-level navigation screens. Sub-screens (Sync, TorSettings,
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
}
