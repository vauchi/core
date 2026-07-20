// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Screen-chrome overlays: every `ScreenModel` leaving `AppEngine` is
//! decorated here (screen-id metadata, offline banner, demo-contact
//! banner, update banner, sync indicator) before frontends see it.

use serde::{Deserialize, Serialize};

use vauchi_core::version::{AppUpdateStatus, unix_secs_to_date_string};

use super::{AppEngine, AppScreen};
use crate::ui::component::{Component, TextStyle};
use crate::ui::screen::{ActionStyle, ScreenAction, ScreenLayout, ScreenModel};

/// Shared action ID for the update link button/banner.
pub(super) const ACTION_OPEN_UPDATE_LINK: &str = "open_update_link";
/// Reserved global-chrome action id: the native top-bar gear forwards
/// this instead of constructing the "Settings" screen name. Resolved
/// to `NavigateTo(Settings)` before per-screen dispatch (CoreScreenIdMap
/// rework Tier-0; ADR-043 Amendment 4 — forward nav is core-resolved).
pub(super) const ACTION_OPEN_SETTINGS: &str = "open_settings";
/// Reserved global-chrome action id: the visible Back affordance. Stamped
/// as a `nav_actions` item wherever `can_go_back()` holds, so frontends
/// render Back from data instead of the `can_go_back` bool (ADR-044 Am2a,
/// boolean-family retirement). Resolved to the same back logic as
/// `UserAction::NavigateBack` before per-screen dispatch.
pub(super) const ACTION_GO_BACK: &str = "go_back";
/// Action id used by the offline `Component::Banner` injected by
/// `apply_offline_overlay`. Currently presentational only — no
/// dispatcher arm. Frontends rendering the banner can ignore taps.
pub(super) const ACTION_OFFLINE_BANNER: &str = "offline_banner";

/// Reserved action id for the demo-contact banner's dismiss button.
/// Emitted on `Component::Banner` from `apply_demo_contact_overlay`;
/// `handle_action` intercepts presses to call
/// `Vauchi::dismiss_demo_contact`. Per ADR-043 / ADR-021: the
/// state→banner mapping lives in core, not in any frontend's view
/// (was iOS `DemoContactCard` rendering a frontend-derived banner
/// from `viewModel.demoContact`).
pub(super) const ACTION_DISMISS_DEMO_CONTACT: &str = "dismiss_demo_contact";

/// Action id for the sync-chrome `Component::Indicator` tap target.
/// Emitted on top-level screens by `apply_sync_chrome_overlay` when
/// the indicator is tappable (Idle or after a Failed attempt).
/// `handle_action` intercepts presses to call `Vauchi::sync`. Replaces
/// iOS's `HomeView.SyncStatusIndicator` (state→icon switch + 4
/// hardcoded English a11y strings — G1 of
/// `2026-05-02-ios-humble-ui-deep-retirement`).
pub(super) const ACTION_SYNC_NOW: &str = "sync_now";

/// Last sync attempt result tracked by `AppEngine` and surfaced as
/// `Component::Indicator` chrome on every top-level screen via
/// `apply_sync_chrome_overlay`. Design: see
/// `_private/docs/designs/2026-05-28-sync-chrome-overlay-design.md`.
///
/// State source is the engine's own bookkeeping (set after each
/// `Vauchi::sync()` call from the `sync_now` handler), not the
/// send-phase worker's `connection_state()` — the design doc walks
/// through why: `Vauchi` does not field a long-lived sync worker
/// (`SendPhase` is per-cycle), and the user-facing "Synced 15:47" /
/// "Sync failed" model maps cleanly onto the existing
/// `VauchiSyncOutcome` return value.
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

impl AppEngine {
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

    /// Stamp `parent_screen_id` and `presentation_kind` onto the
    /// inner-engine's screen model from the AppEngine-level
    /// `AppScreen`. Per `2026-05-01-screen-id-metadata-in-core` G1+G2 —
    /// frontends consume these instead of substring-matching
    /// `screen_id` strings.
    ///
    /// Idempotent: if the inner engine already set non-default values
    /// (it shouldn't, but the contract is clear), they are preserved.
    pub(super) fn apply_screen_id_metadata(&self, mut screen: ScreenModel) -> ScreenModel {
        // Tier-0 (c) narrow collapse: these 5 families back engines that
        // emit per-sub-state `screen_id`s (`contact_list`, `backup_*`, …)
        // that `CoreScreenIdMap` was hand-folding. Stamp the canonical
        // `AppScreen::screen_id()` so frontends get a stable id. Narrow by
        // design — screens outside the set keep their engine id for in-flow
        // render-diffing and internal routing (the `backup_processing`
        // interception runs before this decorator). See the (c) plan.
        if matches!(
            self.screen,
            AppScreen::Contacts | AppScreen::Groups | AppScreen::DuressPin | AppScreen::Backup
        ) {
            screen.screen_id = self.screen.screen_id().to_string();
        } else if matches!(self.screen, AppScreen::Exchange)
            && screen.screen_id == "exchange_mode_selection"
        {
            // Only the mode-selection ROOT carries the canonical `exchange`
            // id so frontends show the nav bar (screen_id == tab_id). The
            // engine's other sub-states (verifying/success/nfc_role) can't
            // join the blanket set above — they keep their ids so the bar
            // hides mid-flow and native wrappers still dispatch. See
            // canonical_screen_id_tests + 2026-06-05-screen-ux-declutter.
            screen.screen_id = AppScreen::Exchange.screen_id().to_string();
        }
        if screen.parent_screen_id.is_none() {
            screen.parent_screen_id = self.screen.parent_screen_id().map(String::from);
        }
        if screen.nav_tab_id.is_none() {
            screen.nav_tab_id = self.screen.nav_tab_id();
        }
        if matches!(
            screen.presentation_kind,
            crate::ui::screen::ScreenPresentationKind::Page
        ) {
            screen.presentation_kind = self.screen.presentation_kind();
        }
        // Back affordance from engine nav state: stamp a reserved `go_back`
        // nav action so frontends render Back from data, never from a
        // per-concept boolean (ADR-044 Am2a).
        if self.can_go_back() && !screen.nav_actions.iter().any(|a| a.id == ACTION_GO_BACK) {
            let label = self.t("nav.back");
            screen.nav_actions.insert(
                0,
                ScreenAction {
                    id: ACTION_GO_BACK.into(),
                    label: label.clone(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(crate::ui::component::A11y::labeled(label)),
                },
            );
        }
        screen
    }

    /// Decorate the given screen with an offline `Component::Banner`
    /// when `network_online == false`. Idempotent — only inserts a
    /// banner; never duplicates one already present.
    ///
    /// Inserted at the bottom of the existing components so an
    /// active update banner (`apply_update_overlay`) keeps its
    /// top-of-screen position.
    /// Transform the rendered screen's design tokens per the user's
    /// Category-2 accessibility flags (M4 S1a; ADR-047 Addendum
    /// 2026-07-05). Reads live config so a mid-session toggle takes
    /// effect on the next render; a no-op when both flags are off, which
    /// keeps golden fixtures on the default tokens.
    pub(super) fn apply_accessibility_overlay(&self, mut screen: ScreenModel) -> ScreenModel {
        let config = self.vauchi().config();
        screen.tokens = crate::theme::apply_accessibility_tokens(
            screen.tokens,
            config.reduce_motion,
            config.large_touch,
        );
        screen
    }

    pub(super) fn apply_offline_overlay(&self, mut screen: ScreenModel) -> ScreenModel {
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
            text: self.t("offline.banner"),
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
    pub(super) fn apply_demo_contact_overlay(&self, mut screen: ScreenModel) -> ScreenModel {
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
    pub(super) fn apply_update_overlay(&self, mut screen: ScreenModel) -> ScreenModel {
        match &self.update_status {
            AppUpdateStatus::UpToDate => screen,
            AppUpdateStatus::UpdateAvailable => {
                if self.update_dismissed {
                    return screen;
                }
                screen.components.insert(
                    0,
                    Component::Banner {
                        text: self.t("update.available_banner"),
                        action_label: self.t("update.available_action"),
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
                        text: crate::i18n::get_string_with_args(
                            self.render_context.resolved_locale(),
                            "update.required_by",
                            &[("date", &date)],
                        ),
                        action_label: self.t("update.available_action"),
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
                self.t("update.required_title"),
                vec![Component::Text {
                    id: "update_message".into(),
                    content: self.t("update.unsupported_message"),
                    style: TextStyle::Body,
                }],
                vec![ScreenAction {
                    id: ACTION_OPEN_UPDATE_LINK.into(),
                    label: self.t("update.update_now_action"),
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
    pub(super) fn apply_sync_chrome_overlay(&self, mut screen: ScreenModel) -> ScreenModel {
        // Fixed-layout screens (e.g. the QR exchange) must not reflow:
        // the sync chrome's state changes would shift a live element —
        // the QR the peer is scanning — and break the camera lock.
        // Skip the overlay there (`2026-06-03-exchange-qr-scan-stability`).
        if screen.layout == ScreenLayout::Fixed {
            return screen;
        }
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
            SyncChromeStatus::Idle => (
                "Sync".to_string(),
                crate::ui::component::IndicatorKind::Neutral,
            ),
            SyncChromeStatus::Synced { .. } => (
                "Synced".to_string(),
                crate::ui::component::IndicatorKind::Active,
            ),
            SyncChromeStatus::Failed => (
                "Sync failed".to_string(),
                crate::ui::component::IndicatorKind::Error,
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

    /// Inject the global Settings chrome action on the home screen. Core
    /// owns *what* chrome actions a screen offers; each frontend presents
    /// `nav_actions` per its form factor (mobile top-bar gear, desktop
    /// sidebar). `open_settings` is resolved to `NavigateTo(Settings)`
    /// before per-screen dispatch, so this retires the native
    /// `ReadyScreen`/`isHomeTab` gear
    /// (`2026-07-06-mobile-domain-shell-violations`).
    pub(super) fn apply_nav_chrome_overlay(&self, mut screen: ScreenModel) -> ScreenModel {
        if screen.screen_id != "my_info" {
            return screen;
        }
        if screen
            .nav_actions
            .iter()
            .any(|a| a.id == ACTION_OPEN_SETTINGS)
        {
            return screen;
        }
        let label = self.t("settings.title");
        screen.nav_actions.push(ScreenAction {
            id: ACTION_OPEN_SETTINGS.into(),
            label: label.clone(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: Some(crate::ui::component::A11y::labeled(label)),
        });
        screen
    }
}
