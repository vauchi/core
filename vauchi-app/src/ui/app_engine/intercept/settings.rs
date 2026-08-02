// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings & consent intercepts: consent-toggle durability, settings/
//! accessibility toggle persistence, and Settings-screen sub-navigation.
//! Split out of `intercept.rs` (cohesion). These are `impl AppEngine`
//! methods, dispatched from `mod.rs`/`dispatch.rs`.

use super::super::AppEngine;
use super::super::AppScreen;
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::form_dialog::FormDialogType;

impl AppEngine {
    /// Persist a consent toggle on the Privacy screen by flipping the
    /// stored grant for the toggled `ConsentType`. The `GdprEngine` flips
    /// its in-memory display in parallel; this owns durability.
    pub(in crate::ui::app_engine) fn persist_consent_toggle(&mut self, action: &UserAction) {
        if self.screen != AppScreen::Privacy {
            return;
        }
        let UserAction::SettingsToggled {
            component_id,
            item_id,
        } = action
        else {
            return;
        };
        if component_id != "consent" {
            return;
        }
        let Some(consent_type) = vauchi_core::api::ConsentType::parse(item_id.as_str()) else {
            return;
        };
        let granted = self
            .vauchi
            .export_consent_log()
            .unwrap_or_default()
            .iter()
            .rfind(|r| r.consent_type == consent_type)
            .map(|r| r.granted)
            .unwrap_or(false);
        let result = if granted {
            self.vauchi.revoke_consent(consent_type)
        } else {
            self.vauchi.grant_consent(consent_type)
        };
        if let Err(err) = result {
            tracing::warn!(?err, "consent toggle persist failed");
        }
    }

    /// Persist settings toggle changes to Vauchi config (fixes HIGH-4).
    pub(in crate::ui::app_engine) fn persist_settings_toggle(&mut self, action: &UserAction) {
        if self.screen != AppScreen::Settings {
            return;
        }
        if let UserAction::SettingsToggled {
            component_id,
            item_id,
        } = action
            && matches!(
                component_id.as_str(),
                // M6 S1b merged privacy+notifications into one group id.
                "privacy_notifications" | "accessibility"
            )
        {
            let config = self.vauchi.config_mut();
            match item_id.as_str() {
                "delivery_receipts" => {
                    config.delivery_receipts_enabled = !config.delivery_receipts_enabled;
                }
                "suppress_presence" => {
                    config.suppress_presence = !config.suppress_presence;
                }
                "new_field_default" => {
                    config.new_field_default_visible = !config.new_field_default_visible;
                }
                "contact_added" => {
                    config.contact_added_notifications = !config.contact_added_notifications;
                }
                "card_update" => {
                    config.card_update_notifications = !config.card_update_notifications;
                }
                // Category-2 accessibility flags (ADR-047 Addendum
                // 2026-07-05) — core-owned so they follow the user.
                "reduce_motion" => {
                    config.reduce_motion = !config.reduce_motion;
                }
                "large_touch" => {
                    config.large_touch = !config.large_touch;
                }
                _ => {}
            }
            // Persist the updated flags to durable core Storage so the choice
            // survives restart and self-seeds config on the next launch
            // (settings-toggle-not-persisting P2). Best-effort, like the
            // backup-reminder arm below: a failed save leaves the previous
            // durable value, the in-memory toggle still reflects in-session.
            let mut flags = self.vauchi.load_settings_flags().unwrap_or_default();
            flags.merge_config_toggles(self.vauchi.config());
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.vauchi.save_settings_flags(&flags);
        }

        // Handle backup_reminders frequency cycling (ListItemSelected, not SettingsToggled)
        if let UserAction::ListItemSelected {
            component_id,
            item_id,
        } = action
            && component_id == "security_backup"
            && item_id == "backup_reminders"
            && let Ok(mut state) = self.vauchi.load_backup_reminder_state()
        {
            let next = state.frequency.next();
            state.frequency = next;
            state.reminders_enabled = next != vauchi_core::types::ReminderFrequency::Never;
            // best-effort: reminder cadence is a UX setting; failure leaves
            // the previous cadence active until the next user change
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.vauchi.save_backup_reminder_state(&state);
        }

        // Handle theme + language Dropdown selections.
        // Component::Dropdown emits UserAction::ListItemSelected with
        // component_id = the dropdown id ("theme" / "language") and
        // item_id = the picked option id. The reserved id "follow_system"
        // means "let the OS decide" — maps to None per ADR-047.
        //
        // S6 of 2026-05-16-settings-storage-by-sensitivity: vault write
        // retired; RenderContext is the single source of truth. The
        // frontend's setRenderContextJson push at boot + on every
        // per-platform OS-native persist (UserDefaults / SharedPreferences)
        // owns the durability.
        if let UserAction::ListItemSelected {
            component_id,
            item_id,
        } = action
            && (component_id == "theme" || component_id == "language")
        {
            let mut ctx = self.render_context.clone();
            let new_value = (item_id != "follow_system").then(|| item_id.clone());
            match component_id.as_str() {
                "theme" => ctx.theme_id = new_value,
                "language" => ctx.locale = new_value,
                _ => unreachable!(),
            }
            self.set_render_context(ctx);
        }
    }

    /// Intercept settings item selection to route to proper sub-screens.
    pub(in crate::ui::app_engine) fn intercept_settings_action(
        &mut self,
        action: &UserAction,
    ) -> Option<ActionResult> {
        if self.screen != AppScreen::Settings {
            return None;
        }
        if let UserAction::ListItemSelected { item_id, .. } = action {
            match item_id.as_str() {
                "display_name" => {
                    let current_name = self
                        .vauchi
                        .own_card()
                        .ok()
                        .flatten()
                        .map(|c| c.display_name().to_string())
                        .unwrap_or_default();
                    let screen = self.navigate_to(AppScreen::FormDialog {
                        dialog_type: FormDialogType::EditName { current_name },
                    });
                    return Some(ActionResult::NavigateTo(screen));
                }
                "edit_profile" => {
                    let screen = self.navigate_to(AppScreen::MyInfo);
                    return Some(ActionResult::NavigateTo(screen));
                }
                "devices" => {
                    let screen = self.navigate_to(AppScreen::DeviceManagement);
                    return Some(ActionResult::NavigateTo(screen));
                }
                "duress_pin" => {
                    let screen = self.navigate_to(AppScreen::DuressPin);
                    return Some(ActionResult::NavigateTo(screen));
                }
                "decoy_contacts" => {
                    let screen = self.navigate_to(AppScreen::DecoyContacts);
                    return Some(ActionResult::NavigateTo(screen));
                }
                "relay_url" => {
                    let current_url = self.vauchi.config().relay.server_url.clone();
                    let screen = self.navigate_to(AppScreen::FormDialog {
                        dialog_type: FormDialogType::EditRelayUrl { current_url },
                    });
                    return Some(ActionResult::NavigateTo(screen));
                }
                "emergency_wipe" => {
                    let screen = self.navigate_to(AppScreen::EmergencyShred);
                    return Some(ActionResult::NavigateTo(screen));
                }
                "backup_export" | "backup_import" => {
                    let screen = self.navigate_to(AppScreen::Backup);
                    return Some(ActionResult::NavigateTo(screen));
                }
                "setup_new_device" => {
                    let screen = self.navigate_to(AppScreen::DeviceReplacement);
                    return Some(ActionResult::NavigateTo(screen));
                }
                // The Sync screen was retired (M4 S2); the chrome sync chip
                // is the sync surface. The Failed Deliveries settings row now
                // links into the DeliveryStatus screen (previously an
                // unreachable orphan — 2026-07-03-sync-surface-placebo).
                "failed_deliveries" => {
                    let screen = self.navigate_to(AppScreen::DeliveryStatus);
                    return Some(ActionResult::NavigateTo(screen));
                }
                "help_center" => {
                    let screen = self.navigate_to(AppScreen::Help);
                    return Some(ActionResult::NavigateTo(screen));
                }
                "funding" => {
                    return Some(ActionResult::OpenUrl {
                        url: "https://vauchi.app/docs/about/supporters".into(),
                    });
                }
                "privacy_policy" => {
                    return Some(ActionResult::OpenUrl {
                        url: "https://vauchi.app/docs/legal/privacy-policy".into(),
                    });
                }
                "change_password" => {
                    let screen = self.navigate_to(AppScreen::ChangePassword);
                    return Some(ActionResult::NavigateTo(screen));
                }
                // M6 D6.1: the "Advanced…" row opens the buried sub-screen
                // (network, delivery status, emergency wipe).
                "advanced" => {
                    let screen = self.navigate_to(AppScreen::SettingsAdvanced);
                    return Some(ActionResult::NavigateTo(screen));
                }
                _ => {}
            }
        }
        None
    }
}
