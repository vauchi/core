// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The emergency broadcast screen's Core calls (ADR-036, private#895).
//! Sending never waits: it is the one-tap act under stress. Changing who
//! receives the alerts waits for the session PIN like the duress PIN does
//! (#781), so someone holding the unlocked phone cannot redirect them.

use super::super::credential_gate::CredentialChange;
use super::{AppEngine, AppScreen};
use crate::i18n::get_string_with_args;
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::emergency_broadcast::{
    CONFIG_ACTIONS_COMPONENT_ID, CONFIRM_DISABLE_ACTION_ID, CONFIRM_SEND_ACTION_ID,
    SAVE_CONFIG_ACTION_ID, SEND_ACTIONS_COMPONENT_ID, SEND_ALERT_ACTION_ID,
};
use crate::ui::emergency_broadcast::{EmergencyBroadcastEngine, TrustedContactChoice};
use crate::ui::{EngineOutput, EngineUpdate, WorkflowEngine};
use vauchi_core::api::Vauchi;

impl AppEngine {
    /// The screen from storage: the saved recipients, message and location
    /// choice, over every unblocked contact.
    pub(in crate::ui::app_engine) fn build_emergency_broadcast_engine(
        vauchi: &Vauchi,
        render_context: &crate::ui::RenderContext,
    ) -> Box<dyn WorkflowEngine> {
        let config = vauchi.load_emergency_config().ok().flatten();
        let trusted: Vec<String> = config
            .as_ref()
            .map(|c| c.trusted_contact_ids.clone())
            .unwrap_or_default();
        let contacts = vauchi
            .list_contacts()
            .unwrap_or_default()
            .into_iter()
            .filter(|c| !c.is_blocked())
            .map(|c| TrustedContactChoice {
                id: c.id().to_string(),
                display_name: c.display_name().to_string(),
                selected: trusted.iter().any(|id| id == c.id()),
            })
            .collect();
        let (message, include_location) = match &config {
            Some(c) => (c.message.clone(), c.include_location),
            None => (
                vauchi_core::emergency::DEFAULT_EMERGENCY_MESSAGE.to_string(),
                false,
            ),
        };
        Box::new(EmergencyBroadcastEngine::new(
            contacts,
            message,
            include_location,
            config.is_some(),
            render_context.resolved_locale(),
        ))
    }

    pub(in crate::ui::app_engine) fn intercept_emergency_broadcast_action(
        &mut self,
        action: &UserAction,
    ) -> Option<ActionResult> {
        if self.screen != AppScreen::EmergencyBroadcast {
            return None;
        }
        match action {
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == SEND_ACTIONS_COMPONENT_ID && item_id == SEND_ALERT_ACTION_ID => {
                let remaining = self.vauchi.emergency_broadcast_cooldown_remaining_secs();
                (remaining > 0).then(|| self.cooldown_alert(remaining))
            }
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == CONFIG_ACTIONS_COMPONENT_ID
                && item_id == SAVE_CONFIG_ACTION_ID =>
            {
                match self.engine.engine_output() {
                    Some(EngineOutput::EmergencyBroadcast {
                        trusted_contact_ids,
                        ..
                    }) if !trusted_contact_ids.is_empty() => Some(
                        self.confirm_session_pin_then(CredentialChange::EmergencyBroadcastConfig),
                    ),
                    // No recipient: the engine shows why and nothing is saved.
                    _ => None,
                }
            }
            UserAction::ActionPressed { action_id } if action_id == CONFIRM_SEND_ACTION_ID => {
                Some(self.send_emergency_alert())
            }
            UserAction::ActionPressed { action_id } if action_id == CONFIRM_DISABLE_ACTION_ID => {
                Some(self.confirm_session_pin_then(CredentialChange::EmergencyBroadcastDisable))
            }
            _ => None,
        }
    }

    fn cooldown_alert(&self, remaining_secs: u64) -> ActionResult {
        ActionResult::ShowAlert {
            title: self.t("emergency.title"),
            message: get_string_with_args(
                self.render_context.resolved_locale(),
                "emergency.cooldown_active",
                &[("seconds", &remaining_secs.to_string())],
            ),
        }
    }

    fn send_emergency_alert(&mut self) -> ActionResult {
        match self.vauchi.send_emergency_broadcast() {
            Ok(result) => {
                self.refresh_emergency_broadcast_engine();
                let _ = self
                    .engine
                    .apply_update(EngineUpdate::EmergencyBroadcastSent {
                        sent: result.sent,
                        total: result.total,
                    });
                ActionResult::UpdateScreen(self.engine.current_screen())
            }
            Err(error) => {
                self.refresh_emergency_broadcast_engine();
                let remaining = self.vauchi.emergency_broadcast_cooldown_remaining_secs();
                if remaining > 0 {
                    return self.cooldown_alert(remaining);
                }
                ActionResult::ShowAlert {
                    title: self.t("emergency_broadcast.send_failed_title"),
                    message: error.to_string(),
                }
            }
        }
    }

    /// Saves the picked recipients, message and location choice; runs
    /// after the session PIN step when a password is set.
    pub(in crate::ui::app_engine) fn complete_emergency_broadcast_config(
        &mut self,
    ) -> ActionResult {
        let Some(EngineOutput::EmergencyBroadcast {
            trusted_contact_ids,
            message,
            include_location,
        }) = self.engine.engine_output()
        else {
            tracing::warn!("emergency broadcast save without EmergencyBroadcast output");
            return ActionResult::UpdateScreen(self.engine.current_screen());
        };
        let message = match message.trim() {
            "" => vauchi_core::emergency::DEFAULT_EMERGENCY_MESSAGE.to_string(),
            trimmed => trimmed.to_string(),
        };
        if let Err(error) = self.vauchi.configure_emergency_broadcast(
            trusted_contact_ids,
            message,
            include_location,
        ) {
            return ActionResult::ShowAlert {
                title: self.t("error.title"),
                message: error.to_string(),
            };
        }
        self.invalidate_screen(&AppScreen::Settings);
        self.refresh_emergency_broadcast_engine();
        ActionResult::UpdateScreen(self.engine.current_screen())
    }

    pub(in crate::ui::app_engine) fn complete_emergency_broadcast_disable(
        &mut self,
    ) -> ActionResult {
        if let Err(error) = self.vauchi.delete_emergency_config() {
            return ActionResult::ShowAlert {
                title: self.t("error.title"),
                message: error.to_string(),
            };
        }
        self.invalidate_screen(&AppScreen::Settings);
        self.refresh_emergency_broadcast_engine();
        ActionResult::UpdateScreen(self.engine.current_screen())
    }

    fn refresh_emergency_broadcast_engine(&mut self) {
        let screen = AppScreen::EmergencyBroadcast;
        self.engine_cache.remove(&screen);
        self.engine = Self::create_engine(
            &self.vauchi,
            &screen,
            self.preview_as_contact.as_deref(),
            &self.device_capabilities,
            &self.transport_readiness,
            &self.render_context,
            &self.pending_exchange_groups,
            self.glance_display_qr.as_deref(),
            &self.ble_attempt_token,
        );
    }
}
