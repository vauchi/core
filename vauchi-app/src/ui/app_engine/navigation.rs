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
            Self::ContactDuplicates | Self::ContactLimit | Self::ArchivedContacts => Self::Contacts,
            Self::MyInfoEntryDetail { .. } | Self::AvatarEditor => Self::MyInfo,
            Self::GroupDetail { .. } => Self::Groups,
            Self::RecoveryHelp | Self::RecoveryClaimReview => Self::Recovery,
            Self::DeviceLinking | Self::DeviceReplacement => Self::DeviceManagement,
            Self::FormDialog { .. } => return None,
            Self::Lock => return None,
            // Deep-link consent + responder + device-link join are modal-shaped — no parent tab.
            Self::DeepLinkConsent { .. }
            | Self::DeepLinkResponder { .. }
            | Self::DeviceLinkJoin { .. } => return None,
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
                | Self::More
                | Self::Onboarding => canonical,
                // Settings sub-flows — collapse under Settings on
                // desktop (no More tab in the desktop sidebar).
                Self::DuressPin | Self::ChangePassword | Self::EmergencyShred => Self::Settings,
                // Exchange-side sync indicator.
                Self::DeliveryStatus => Self::Exchange,
                // Multi-stage face-to-face exchange — collapses under
                // Exchange on every layout (it's an active sub-flow,
                // not a top-level destination).
                Self::MultiStageExchange { .. } => Self::Exchange,
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
                | Self::DuressPin
                | Self::ChangePassword
                | Self::EmergencyShred => Self::More,
                Self::DeliveryStatus => Self::Exchange,
                Self::MultiStageExchange { .. } => Self::Exchange,
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

    /// Set the initial screen during frontend bootstrap **without**
    /// pushing the prior screen to nav history.
    ///
    /// `AppEngine::new` initializes `screen = AppScreen::Onboarding`
    /// when no identity exists. Frontends that detect identity at
    /// startup (TUI, iOS, Android) then need to swap to MyInfo / Lock /
    /// Contacts. Using `navigate_to` for that swap pollutes
    /// `nav_history` with a stale Onboarding entry, so the user's
    /// first `navigate_back` lands on Onboarding instead of the
    /// expected parent. This method is the bootstrap-only entry point.
    ///
    /// **Do not call this for user-driven navigation.** Use
    /// `navigate_to` for that.
    pub fn set_initial_screen(&mut self, screen: AppScreen) -> ScreenModel {
        self.navigate_to_internal(screen)
    }

    /// Resolve and apply the bootstrap screen from Core-owned state.
    ///
    /// Prefer this over `set_initial_screen` in shells. Choosing between
    /// onboarding, lock and the default landing screen is a navigation
    /// decision derived from domain state (identity presence, password
    /// enablement), and ADR-066 reserves those to Core — a shell that
    /// derives it must import `AppScreen`, which is retired from the shell
    /// boundary. The TUI did exactly that before this existed.
    pub fn bootstrap(&mut self) -> ScreenModel {
        let screen = if !self.vauchi().has_identity() {
            AppScreen::Onboarding
        } else if self.vauchi().is_password_enabled().unwrap_or(false) {
            AppScreen::Lock
        } else {
            self.default_screen()
        };
        self.set_initial_screen(screen)
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
        self.retained_detail_screen = screen.parent_screen_id().is_some().then(|| screen.clone());

        // Swap in the new screen, get the old one back
        let old_screen = std::mem::replace(&mut self.screen, screen.clone());

        // Glance's one-sided QR must exist BEFORE `create_engine` builds the
        // engine that renders it — the post-build lifecycle sync below runs too
        // late. Generate it ONCE on entry (a stable nonce for the whole attempt;
        // `BleExchange` is not cacheable, so each entry rebuilds).
        if old_screen != screen
            && let AppScreen::BleExchange { mode } = &screen
            && *mode == vauchi_core::exchange::mode::ExchangeMode::Glance
        {
            self.glance_display_qr = self.begin_glance_display();
        }

        // Build or restore the engine for the new screen.
        // Pass preview_as_contact so MyInfo is built in PreviewAs mode when active.
        let preview_as = self.preview_as_contact.as_deref();
        let new_engine = self.engine_cache.remove(&screen).unwrap_or_else(|| {
            Self::create_engine(
                &self.vauchi,
                &screen,
                preview_as,
                &self.device_capabilities,
                &self.transport_readiness,
                &self.render_context,
                &self.pending_exchange_groups,
                self.glance_display_qr.as_deref(),
            )
        });

        // Swap in the new engine, get the old one back
        let mut old_engine = std::mem::replace(&mut self.engine, new_engine);

        // Entering the Exchange screen afresh starts a new legacy-QR exchange:
        // clear the persist-at-Complete guard so the next completion persists.
        // Sub-steps (mode → group → field preview) stay under `Exchange`, so
        // this only fires on a genuine (re)entry, never mid-flow
        // (2026-06-04-exchange-terminal-screens).
        if old_screen != screen && matches!(screen, AppScreen::Exchange) {
            self.legacy_exchange_persisted = None;
        }

        // Phase 2b: drive screen-presentation lifecycle hooks. The
        // outgoing engine emits its exit `Command`s (e.g. restore
        // brightness), then the incoming engine emits its entry
        // `Command`s (e.g. dim brightness for QR contrast). Both
        // accumulate in the AppEngine's `pending_commands` queue
        // which the frontend drains via `drain_pending_commands()`.
        // Same-screen swaps (which happen e.g. when an engine cache
        // miss rebuilds the same screen) skip both hooks — the
        // platform state should not flap.
        if old_screen != screen {
            self.pending_commands.extend(old_engine.screen_exited());
            self.pending_commands.extend(self.engine.screen_entered());
            // Slice 32l Phase 2: build / drop the engine-owned link-mode
            // responder machine on entry / exit of the responder screen.
            // Its initial deposit commands ride out on the same drain.
            self.sync_link_responder_lifecycle(&old_screen, &screen);
            // Slice 32l Phase 3: build / drop the engine-owned link-mode
            // initiator machine on entry / exit of the LinkExchange screen.
            self.sync_link_initiator_lifecycle(&old_screen, &screen);
            // Slice 32l T3.1b: build / drop the engine-owned device-link
            // initiator machine on entry / exit of the DeviceLinking screen.
            #[cfg(all(feature = "network-http", feature = "storage"))]
            self.sync_device_link_lifecycle(&old_screen, &screen);
            // M5 B3 Slice 3: tear down the engine-owned device-link join
            // (responder) machine on exit of the DeviceLinkJoin screen.
            #[cfg(all(feature = "network-http", feature = "storage"))]
            self.sync_device_link_responder_lifecycle(&old_screen, &screen);
            // Slice 32m T1.2b: build / drop the engine-owned multi-stage
            // machine on entry / exit of the MultiStageExchange screen.
            // T1.2c removes the parallel cycle-thread bridge in
            // PlatformAppEngine to avoid double-driving the engine on
            // mobile.
            self.sync_multi_stage_lifecycle(&old_screen, &screen);
            // BLE/Magic completion P2: tear down the engine-owned BLE
            // handshake machine on exit of the BleExchange screen (the
            // session is built lazily on peer discovery, not on entry,
            // because its initiator/responder role is unknown until the
            // tiebreak tokens are compared).
            self.sync_ble_handshake_lifecycle(&old_screen, &screen);
        }

        // Cache the old engine if its screen is cacheable
        if Self::is_cacheable(&old_screen) {
            self.engine_cache.insert(old_screen, old_engine);
        }

        // Decorate the inner engine's screen for the wire (Tier-0 (c)
        // canonical `screen_id` stamp + parent_screen_id/presentation_kind)
        // so the direct `navigate_to` / `navigate_back` / `set_initial_screen`
        // returns — which CABI/UniFFI frontends read — agree with
        // `current_screen()`. Idempotent: the route_result path re-decorates
        // via `apply_update_overlay_to_result`.
        self.apply_screen_id_metadata(self.engine.current_screen())
    }

    pub(super) fn activate_surface_engine(&mut self, screen: AppScreen) {
        if self.screen == screen {
            return;
        }
        let new_engine = self.engine_cache.remove(&screen).unwrap_or_else(|| {
            Self::create_engine(
                &self.vauchi,
                &screen,
                self.preview_as_contact.as_deref(),
                &self.device_capabilities,
                &self.transport_readiness,
                &self.render_context,
                &self.pending_exchange_groups,
                self.glance_display_qr.as_deref(),
            )
        });
        let old_screen = std::mem::replace(&mut self.screen, screen);
        let old_engine = std::mem::replace(&mut self.engine, new_engine);
        if Self::is_cacheable(&old_screen) {
            self.engine_cache.insert(old_screen, old_engine);
        }
    }

    /// Navigate back using the history stack. Falls back to Home if empty.
    ///
    /// The active engine gets first refusal: engines that host a
    /// multi-step flow under a single `AppScreen` (the exchange flow)
    /// rewind one internal step here, keeping the *same* engine instance
    /// and re-rendering it. Only when the engine reports it is at its
    /// root step do we pop the AppScreen `nav_history`. Without this, a
    /// BACK from an exchange sub-step would tear down the whole Exchange
    /// screen instead of stepping back to mode selection.
    pub fn navigate_back(&mut self) -> ScreenModel {
        if self.engine.navigate_back_within() {
            // Re-decorate the same engine's screen exactly as
            // `navigate_to_internal` does for its final return, so the
            // `screen_id` / parent metadata stays consistent. No engine
            // rebuild, no `nav_history` mutation, no lifecycle hooks.
            return self.apply_screen_id_metadata(self.engine.current_screen());
        }
        let target = self.nav_history.pop().unwrap_or(AppScreen::MyInfo);
        self.navigate_to_internal(target)
    }

    /// Whether the user should be offered a back affordance from the
    /// current screen.
    ///
    /// Frontends query this for their back affordance / `BackHandler`
    /// instead of inferring "is this a core-driven screen?" from a
    /// frontend-side screen-id map (ADR-043: no constructed nav targets).
    ///
    /// The rule is a property of the current screen, not just history:
    /// declared roots (`AppScreen::is_root`) are back-stoppers — back at
    /// a root must exit, not pop `nav_history`. This makes the
    /// post-onboarding handoff safe (crumbs left in history at MyInfo
    /// don't produce a phantom back arrow) without mutating history,
    /// so `navigate_back` itself remains unchanged.
    pub fn can_go_back(&self) -> bool {
        // Engine-internal step history (exchange sub-flow) makes BACK
        // available even at an AppScreen root: the flow lives under one
        // screen but carries its own back-stack.
        self.engine.can_navigate_back_within()
            || (!self.screen.is_root() && !self.nav_history.is_empty())
    }

    /// Screens that should never be cached — always start fresh.
    /// Onboarding IS cacheable: user navigates to FormDialog (add field)
    /// and back, must return to their current step with accumulated data.
    ///
    /// `DirectTransport`, `BleExchange`, `NfcExchange` and
    /// `MultiStageExchange` are excluded: each owns a live hardware
    /// exchange engine with a `tick`-driven stall deadline stamped at
    /// construction / step entry. Caching one across a navigate-away
    /// would carry a stale timestamp and fire a spurious timeout on
    /// return; a fresh engine restarts the handshake cleanly (hardware
    /// can't be paused)
    /// (2026-06-11-exchange-waits-forever-without-capabilities).
    /// `MultiStageExchange` additionally latches `cancelled` on Cancel,
    /// so a cached engine revives as an un-repaintable zombie frozen on
    /// its mid-transfer chrome
    /// (2026-07-02-multistage-zombie-engine-across-mode-reentry).
    fn is_cacheable(screen: &AppScreen) -> bool {
        !matches!(
            screen,
            AppScreen::Lock
                | AppScreen::FormDialog { .. }
                | AppScreen::DeepLinkConsent { .. }
                | AppScreen::DeepLinkResponder { .. }
                | AppScreen::DeviceLinkJoin { .. }
                | AppScreen::DirectTransport
                | AppScreen::BleExchange { .. }
                | AppScreen::NfcExchange
                | AppScreen::MultiStageExchange { .. }
        )
    }

    /// Invalidates a cached engine for a specific screen.
    /// Next navigation to this screen will create a fresh engine.
    /// When the invalidated screen IS the current one, the live engine is
    /// rebuilt too — eviction alone would leave the user parked on a stale
    /// snapshot until a navigate-away-and-back round trip
    /// (2026-07-01-android-contacts-list-stale-after-mutation).
    pub fn invalidate_screen(&mut self, screen: &AppScreen) {
        self.engine_cache.remove(screen);
        if *screen == self.screen {
            self.engine = Self::create_engine(
                &self.vauchi,
                &self.screen,
                self.preview_as_contact.as_deref(),
                &self.device_capabilities,
                &self.transport_readiness,
                &self.render_context,
                &self.pending_exchange_groups,
                self.glance_display_qr.as_deref(),
            );
        }
    }

    /// Invalidates all cached engines. Use after bulk mutations.
    /// Rebuilds the live current engine too, same as `invalidate_screen`.
    pub fn invalidate_all(&mut self) {
        self.engine_cache.clear();
        self.invalidate_screen(&self.screen.clone());
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
            action_id: screen.screen_id().to_string(),
            label,
            icon: icon.to_string(),
            badge_count: 0,
        }
    }
}
