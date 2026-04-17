// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device management engine — lists linked devices with revoke confirmation.
//!
//! Replaces the TUI-local revoke overlay with a core-driven `InlineConfirm`
//! component per ADR-022 (irrevocable actions require InlineConfirm).

use crate::ui::*;

/// Summary info for a linked device (mirrors `vauchi-core::api::DeviceInfo`).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DeviceListItem {
    pub device_index: u32,
    pub device_name: String,
    pub public_key_prefix: String,
    pub is_current: bool,
    pub is_active: bool,
}

/// Engine that displays the device list and handles revocation.
pub struct DeviceManagementEngine {
    devices: Vec<DeviceListItem>,
    pending_revoke_index: Option<u32>,
    /// Set after the user confirms revocation. Read by app engine in `handle_completion`.
    confirmed_revoke_index: Option<u32>,
}

impl DeviceManagementEngine {
    pub fn new(devices: Vec<DeviceListItem>) -> Self {
        Self {
            devices,
            pending_revoke_index: None,
            confirmed_revoke_index: None,
        }
    }

    /// Returns the device index that the user confirmed for revocation.
    pub fn confirmed_revoke_index(&self) -> Option<u32> {
        self.confirmed_revoke_index
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components = Vec::new();

        let items: Vec<ActionListItem> = self
            .devices
            .iter()
            .map(|d| {
                let detail = if d.is_current {
                    Some("This device".into())
                } else if !d.is_active {
                    Some("Revoked".into())
                } else {
                    Some(format!("ID: {}", d.public_key_prefix))
                };
                ActionListItem {
                    id: format!("device:{}", d.device_index),
                    label: d.device_name.clone(),
                    icon: if d.is_current {
                        Some("device.current".into())
                    } else {
                        Some("device".into())
                    },
                    detail,
                    a11y: Some(A11y {
                        label: Some(d.device_name.clone()),
                        hint: if d.is_current {
                            Some("This is your current device.".into())
                        } else if !d.is_active {
                            Some("This device has been revoked.".into())
                        } else {
                            Some("Tap to revoke this device.".into())
                        },
                        role: None,
                    }),
                    info_key: None,
                }
            })
            .collect();

        components.push(Component::ActionList {
            id: "device_list".into(),
            items,
        });

        if let Some(index) = self.pending_revoke_index {
            let name = self
                .devices
                .iter()
                .find(|d| d.device_index == index)
                .map(|d| d.device_name.as_str())
                .unwrap_or("device");
            components.push(Component::InlineConfirm {
                id: format!("revoke_device:{}", index),
                warning: format!(
                    "Revoke '{}'? This device will lose access and must be re-linked.",
                    name
                ),
                confirm_text: "Revoke".into(),
                cancel_text: "Cancel".into(),
                destructive: true,
                a11y: Some(A11y {
                    label: Some(format!("Confirm revoke {}", name)),
                    hint: Some(
                        "This device will lose access and must be re-linked to use Vauchi again."
                            .into(),
                    ),
                    role: Some(AccessibilityRole::Alert),
                }),
            });
        }

        let can_revoke = self.devices.iter().any(|d| !d.is_current && d.is_active);

        ScreenModel {
            screen_id: "device_management".into(),
            title: "Devices".into(),
            subtitle: None,
            components,
            actions: vec![
                ScreenAction {
                    id: "link_device".into(),
                    label: "Link New Device".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "revoke_device".into(),
                    label: "Revoke Device".into(),
                    style: ActionStyle::Secondary,
                    enabled: can_revoke,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for DeviceManagementEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { ref action_id } if action_id == "link_device" => {
                ActionResult::StartDeviceLink
            }
            UserAction::ActionPressed { ref action_id } if action_id == "revoke_device" => {
                // Find the first revocable device (selected device in TUI)
                // The selection is driven by ListItemSelected action
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ListItemSelected {
                ref component_id,
                ref item_id,
            } if component_id == "device_list" => {
                if let Some(idx_str) = item_id.strip_prefix("device:")
                    && let Ok(idx) = idx_str.parse::<u32>()
                {
                    // Check if revocable
                    if let Some(device) = self.devices.iter().find(|d| d.device_index == idx) {
                        if device.is_current {
                            return ActionResult::ShowToast {
                                message: "Cannot revoke the current device".into(),
                                undo_action_id: None,
                            };
                        }
                        if !device.is_active {
                            return ActionResult::ShowToast {
                                message: "Device is already revoked".into(),
                                undo_action_id: None,
                            };
                        }
                        self.pending_revoke_index = Some(idx);
                        return ActionResult::UpdateScreen(self.build_screen());
                    }
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { ref action_id }
                if action_id.starts_with("confirm_revoke_device:") =>
            {
                if let Some(idx_str) = action_id.strip_prefix("confirm_revoke_device:")
                    && let Ok(idx) = idx_str.parse::<u32>()
                {
                    self.pending_revoke_index = None;
                    self.confirmed_revoke_index = Some(idx);
                    return ActionResult::Complete;
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { ref action_id }
                if action_id.starts_with("cancel_revoke_device:") =>
            {
                self.pending_revoke_index = None;
                ActionResult::UpdateScreen(self.build_screen())
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

// INLINE_TEST_REQUIRED: Tests access private pending_revoke_index field and build_screen()
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_devices() -> Vec<DeviceListItem> {
        vec![
            DeviceListItem {
                device_index: 0,
                device_name: "iPhone".into(),
                public_key_prefix: "a1b2c3d4".into(),
                is_current: true,
                is_active: true,
            },
            DeviceListItem {
                device_index: 1,
                device_name: "Desktop".into(),
                public_key_prefix: "e5f6a7b8".into(),
                is_current: false,
                is_active: true,
            },
            DeviceListItem {
                device_index: 2,
                device_name: "Old Phone".into(),
                public_key_prefix: "c9d0e1f2".into(),
                is_current: false,
                is_active: false,
            },
        ]
    }

    // @internal
    #[test]
    fn screen_shows_all_devices() {
        let engine = DeviceManagementEngine::new(sample_devices());
        let screen = engine.build_screen();
        assert_eq!(screen.screen_id, "device_management");

        let device_count = screen
            .components
            .iter()
            .filter_map(|c| match c {
                Component::ActionList { items, .. } => Some(items.len()),
                _ => None,
            })
            .sum::<usize>();
        assert_eq!(device_count, 3, "All 3 devices should be listed");
    }

    // @internal
    #[test]
    fn current_device_shows_this_device_detail() {
        let engine = DeviceManagementEngine::new(sample_devices());
        let screen = engine.build_screen();

        let iphone = find_device_item(&screen, 0);
        assert_eq!(
            iphone.unwrap().detail.as_deref(),
            Some("This device"),
            "Current device must show 'This device'"
        );
    }

    // @internal
    #[test]
    fn revoked_device_shows_revoked_detail() {
        let engine = DeviceManagementEngine::new(sample_devices());
        let screen = engine.build_screen();

        let old = find_device_item(&screen, 2);
        assert_eq!(
            old.unwrap().detail.as_deref(),
            Some("Revoked"),
            "Revoked device must show 'Revoked'"
        );
    }

    // @internal
    #[test]
    fn revoke_action_disabled_when_no_revocable_devices() {
        let devices = vec![DeviceListItem {
            device_index: 0,
            device_name: "Only Device".into(),
            public_key_prefix: "aabbccdd".into(),
            is_current: true,
            is_active: true,
        }];
        let engine = DeviceManagementEngine::new(devices);
        let screen = engine.build_screen();

        let revoke_action = screen.actions.iter().find(|a| a.id == "revoke_device");
        assert!(
            revoke_action.is_some_and(|a| !a.enabled),
            "Revoke action must be disabled when only current device exists"
        );
    }

    // @internal
    #[test]
    fn selecting_revocable_device_shows_inline_confirm() {
        let mut engine = DeviceManagementEngine::new(sample_devices());
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "device_list".into(),
            item_id: "device:1".into(),
        });

        assert_eq!(engine.pending_revoke_index, Some(1));
        let screen = engine.build_screen();
        assert!(
            screen.components.iter().any(|c| matches!(
                c,
                Component::InlineConfirm { id, destructive: true, .. }
                    if id == "revoke_device:1"
            )),
            "Must show InlineConfirm for the selected device"
        );
    }

    // @internal
    #[test]
    fn selecting_current_device_shows_toast() {
        let mut engine = DeviceManagementEngine::new(sample_devices());
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "device_list".into(),
            item_id: "device:0".into(),
        });

        assert!(
            matches!(
                result,
                ActionResult::ShowToast { ref message, .. }
                    if message.contains("Cannot revoke")
            ),
            "Selecting current device must show cannot-revoke toast"
        );
        assert_eq!(engine.pending_revoke_index, None);
    }

    // @internal
    #[test]
    fn selecting_already_revoked_device_shows_toast() {
        let mut engine = DeviceManagementEngine::new(sample_devices());
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "device_list".into(),
            item_id: "device:2".into(),
        });

        assert!(
            matches!(
                result,
                ActionResult::ShowToast { ref message, .. }
                    if message.contains("already revoked")
            ),
            "Selecting revoked device must show already-revoked toast"
        );
    }

    // @internal
    #[test]
    fn confirm_revoke_returns_complete_with_input() {
        let mut engine = DeviceManagementEngine::new(sample_devices());
        engine.pending_revoke_index = Some(1);

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "confirm_revoke_device:1".into(),
        });

        assert!(
            matches!(result, ActionResult::Complete),
            "Confirming revoke must return Complete"
        );
        assert_eq!(
            engine.confirmed_revoke_index(),
            Some(1),
            "Confirmed revoke index must be set"
        );
        assert_eq!(engine.pending_revoke_index, None);
    }

    // @internal
    #[test]
    fn cancel_revoke_clears_pending_state() {
        let mut engine = DeviceManagementEngine::new(sample_devices());
        engine.pending_revoke_index = Some(1);

        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "cancel_revoke_device:1".into(),
        });

        assert_eq!(
            engine.pending_revoke_index, None,
            "Cancel must clear pending revoke"
        );
    }

    // @internal
    #[test]
    fn inline_confirm_warning_includes_device_name() {
        let mut engine = DeviceManagementEngine::new(sample_devices());
        engine.pending_revoke_index = Some(1);

        let screen = engine.build_screen();
        let confirm = screen.components.iter().find_map(|c| match c {
            Component::InlineConfirm { warning, .. } => Some(warning),
            _ => None,
        });

        assert!(
            confirm.is_some_and(|w| w.contains("Desktop")),
            "Warning must mention the device name"
        );
    }

    fn find_device_item(screen: &ScreenModel, index: u32) -> Option<&ActionListItem> {
        let target_id = format!("device:{}", index);
        screen
            .components
            .iter()
            .filter_map(|c| match c {
                Component::ActionList { items, .. } => Some(items),
                _ => None,
            })
            .flatten()
            .find(|item| item.id == target_id)
    }
}
