// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine factories for the security screens: Recovery, Duress PIN and
//! Backup (#459), split from `screens.rs` to keep it under the file-size
//! limit.

use super::AppEngine;
use super::AppScreen;
use crate::ui::backup_recovery::BackupRecoveryEngine;
use crate::ui::component::Item;
use crate::ui::duress_pin::{DuressConfig, DuressPinEngine};
use crate::ui::engine::WorkflowEngine;
use crate::ui::recovery_status::RecoveryEngine;
use vauchi_core::api::Vauchi;

impl AppEngine {
    pub(super) fn create_security_engine(
        vauchi: &Vauchi,
        screen: &AppScreen,
        render_context: &crate::ui::RenderContext,
    ) -> Box<dyn WorkflowEngine> {
        match screen {
            AppScreen::Recovery => {
                // Only contacts marked "Trust for recovery", against core's
                // configured threshold (#459); the screen used to count
                // every contact against a hard-coded 3.
                let readiness = vauchi.get_recovery_readiness().ok();
                let trusted_ids: std::collections::HashSet<String> = vauchi
                    .list_contacts()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|c| c.is_recovery_trusted())
                    .map(|c| c.id().to_string())
                    .collect();
                let contacts: Vec<Item> =
                    Self::load_contact_items(vauchi, render_context.resolved_locale())
                        .into_iter()
                        .map(|c| c.item)
                        .filter(|item| trusted_ids.contains(&item.id))
                        .collect();
                let threshold = readiness.map_or(3, |r| r.threshold as usize);
                let device_count = vauchi
                    .list_devices()
                    .map(|d| d.len().saturating_sub(1))
                    .unwrap_or(0);
                let mut engine = RecoveryEngine::new(contacts, threshold)
                    .with_locale(render_context.resolved_locale());
                engine.set_linked_device_count(device_count);
                Box::new(engine)
            }
            AppScreen::DuressPin => {
                // Load ALL contacts as the picker pool (even with no stored
                // settings) so a recipient can be chosen (config-gaps defect 1).
                let available_contacts = Self::picker_contacts(vauchi);
                let settings = vauchi.load_duress_settings().ok().flatten();
                // Set up means a duress PIN exists, with or without alerts;
                // it used to mean "alert settings exist" (#459).
                let enabled = vauchi.is_duress_enabled().unwrap_or(false) || settings.is_some();
                let (selected_contact_ids, alert_message, include_location) = match settings {
                    Some(s) => (s.alert_contact_ids, s.alert_message, s.include_location),
                    None => (Vec::new(), String::new(), false),
                };
                let decoy_count = vauchi.list_decoy_contacts().map_or(0, |d| d.len());
                Box::new(
                    DuressPinEngine::new(
                        DuressConfig {
                            enabled,
                            available_contacts,
                            selected_contact_ids,
                            alert_message,
                            include_location,
                        },
                        render_context.resolved_locale(),
                    )
                    .with_decoy_count(decoy_count),
                )
            }
            AppScreen::Backup => Box::new(
                BackupRecoveryEngine::new(
                    None,
                    vauchi.has_identity(),
                    render_context.resolved_locale(),
                )
                .with_last_backup(
                    vauchi
                        .load_backup_reminder_state()
                        .ok()
                        .and_then(|s| s.last_backup_timestamp),
                    vauchi.clock().unix_seconds(),
                ),
            ),
            _ => unreachable!("create_security_engine called for {screen:?}"),
        }
    }
}
