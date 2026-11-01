// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The "what we store" screen (private#86): Settings → Privacy & Data →
//! View My Data. What this device holds is counted when the screen opens;
//! what the relay holds is described, not counted — the relay-side listing
//! is private#631.

use super::*;
#[cfg(feature = "storage")]
use vauchi_core::api::Vauchi;

const SECURITY_REVIEW_URL: &str = "https://vauchi.app/docs/security/previous-analysis";

/// What this device holds, counted when the screen opens.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DataInventory {
    pub card_field_count: usize,
    pub contact_count: usize,
    pub device_count: usize,
    pub pending_update_count: usize,
    /// The database file's size; `None` for an in-memory database.
    pub size_on_disk: Option<u64>,
}

#[cfg(feature = "storage")]
impl DataInventory {
    /// A count that fails reads as zero: the screen is a disclosure of what
    /// is here, and a storage error must not make it disappear.
    pub fn collect(vauchi: &Vauchi) -> Self {
        let storage = vauchi.storage();
        Self {
            card_field_count: vauchi
                .own_card()
                .ok()
                .flatten()
                .map(|card| card.fields().len())
                .unwrap_or(0),
            contact_count: vauchi.contact_count().unwrap_or(0),
            device_count: vauchi
                .list_devices()
                .map(|devices| devices.iter().filter(|d| d.is_active).count())
                .unwrap_or(0),
            pending_update_count: storage.pending().count_all_pending_updates().unwrap_or(0),
            size_on_disk: storage
                .db_path()
                .and_then(|path| std::fs::metadata(path).ok())
                .map(|metadata| metadata.len()),
        }
    }
}

impl GdprEngine {
    /// What this device holds, for the View My Data screen.
    pub fn with_data_inventory(mut self, inventory: DataInventory) -> Self {
        self.inventory = inventory;
        self
    }

    pub(super) fn handle_my_data(&mut self, action: &UserAction) -> Option<ActionResult> {
        match (&self.step, action) {
            (GdprStep::Overview, UserAction::ListItemSelected { item_id, .. })
                if item_id == "view_data" =>
            {
                self.step = GdprStep::MyData;
                Some(ActionResult::NavigateTo(self.build_screen()))
            }
            (GdprStep::MyData, UserAction::ActionPressed { action_id })
                if action_id == "open_security_review" =>
            {
                Some(ActionResult::OpenUrl {
                    url: SECURITY_REVIEW_URL.into(),
                })
            }
            (GdprStep::MyData, UserAction::ActionPressed { action_id })
                if action_id == "cancel" =>
            {
                self.step = GdprStep::Overview;
                Some(ActionResult::NavigateTo(self.build_screen()))
            }
            _ => None,
        }
    }

    pub(super) fn build_my_data(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "my_data".into(),
            title: self.t("privacy.my_data.title"),
            subtitle: Some(self.t("privacy.my_data.subtitle")),
            components: vec![
                Component::InfoPanel {
                    id: "on_this_device".into(),
                    icon: Some("devices".into()),
                    title: self.t("privacy.my_data.device_title"),
                    items: self.device_rows(),
                    a11y: None,
                },
                Component::InfoPanel {
                    id: "on_the_relay".into(),
                    icon: Some("shield".into()),
                    title: self.t("privacy.my_data.relay_title"),
                    items: vec![
                        self.row("privacy.my_data.relay_envelopes"),
                        self.row("privacy.my_data.relay_operator"),
                        self.row("privacy.my_data.nowhere_else"),
                    ],
                    a11y: None,
                },
                Component::StatusIndicator {
                    id: "encryption_status".into(),
                    icon: Some("checkmark.circle.fill".into()),
                    title: self.t("privacy.my_data.encrypted"),
                    detail: Some(self.t("privacy.my_data.encrypted_detail")),
                    status: Status::Success,
                    status_label: self.t("privacy.my_data.encrypted_label"),
                    a11y: None,
                },
            ],
            contextual_actions: vec![
                ScreenAction {
                    id: "open_security_review".into(),
                    label: self.t("privacy.my_data.security_review"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(
                        self.t("privacy.my_data.security_review_a11y"),
                    )),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.back"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.back"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn device_rows(&self) -> Vec<InfoItem> {
        let inventory = &self.inventory;
        let mut rows = vec![
            self.counted_row("privacy.my_data.identity", inventory.card_field_count),
            self.counted_row("privacy.my_data.contacts", inventory.contact_count),
            self.counted_row("privacy.my_data.devices", inventory.device_count),
            self.counted_row("privacy.my_data.outgoing", inventory.pending_update_count),
            self.row("privacy.my_data.settings"),
        ];
        if let Some(bytes) = inventory.size_on_disk {
            rows.push(InfoItem {
                icon: None,
                title: self.t("privacy.my_data.size"),
                detail: get_string_with_args(
                    self.locale,
                    "privacy.my_data.size_detail",
                    &[("size", &format_size(bytes))],
                ),
            });
        }
        rows
    }

    /// A row whose detail lives under `<key>_detail`.
    fn row(&self, key: &str) -> InfoItem {
        InfoItem {
            icon: None,
            title: self.t(key),
            detail: self.t(&format!("{key}_detail")),
        }
    }

    fn counted_row(&self, key: &str, count: usize) -> InfoItem {
        InfoItem {
            icon: None,
            title: self.t(key),
            detail: self.t_count(&format!("{key}_detail"), count),
        }
    }
}

/// Whole kilobytes below a megabyte, one decimal above: the row says
/// "about", so finer precision would claim more than a WAL-mode file size
/// can promise.
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * KB;
    if bytes < MB {
        format!("{} KB", bytes.div_ceil(KB))
    } else {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    }
}
