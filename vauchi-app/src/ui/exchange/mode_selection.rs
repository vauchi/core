// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mode selection engine — shows all exchange modes grouped by
//! category with availability and recommendation.
//!
//! Wired into `ExchangeEngine` in Phase 1.2.

use crate::ui::*;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::{ExchangeMode, ModeCategory};
use vauchi_core::exchange::mode_availability::{
    ModeAvailability, check_mode_availability, recommend_mode,
};

/// Engine that displays exchange mode selection.
///
/// Shows all 9 modes grouped by `ModeCategory`, highlights the
/// recommended mode, and grays out unavailable modes with a reason.
/// When the user picks a mode, returns `ModeSelectionResult::Selected`.
pub struct ModeSelectionEngine {
    capabilities: DeviceCapabilities,
    recommended: ExchangeMode,
}

/// Result of handling an action in the mode selection engine.
pub enum ModeSelectionResult {
    /// User selected a mode.
    Selected(ExchangeMode),
    /// Screen update (e.g., unknown action).
    Screen(Box<ScreenModel>),
}

/// Display-order categories with human-readable labels.
const CATEGORY_ORDER: &[(ModeCategory, &str)] = &[
    (ModeCategory::Quick, "Quick"),
    (ModeCategory::Standard, "Standard"),
    (ModeCategory::Fun, "Fun"),
    (ModeCategory::Remote, "Remote"),
];

impl ModeSelectionEngine {
    pub fn new(capabilities: DeviceCapabilities) -> Self {
        let recommended = recommend_mode(&capabilities);
        Self {
            capabilities,
            recommended,
        }
    }

    /// Build the mode selection screen.
    pub fn screen(&self) -> ScreenModel {
        let components: Vec<Component> = CATEGORY_ORDER
            .iter()
            .filter_map(|(category, label)| {
                let items: Vec<ActionListItem> = ExchangeMode::all()
                    .iter()
                    .filter(|m| m.category() == *category)
                    .map(|&mode| {
                        let availability = check_mode_availability(mode, &self.capabilities);
                        let is_recommended = mode == self.recommended;
                        let icon = if is_recommended {
                            Some("star.fill".into())
                        } else {
                            None
                        };
                        let detail = match &availability {
                            ModeAvailability::Available => None,
                            ModeAvailability::Degraded { reason }
                            | ModeAvailability::Unavailable { reason } => Some(reason.clone()),
                            _ => None,
                        };
                        ActionListItem {
                            id: format!("mode:{}", mode.serde_name()),
                            label: mode.display_name().to_string(),
                            icon,
                            detail,
                            a11y: None,
                            info_key: None,
                        }
                    })
                    .collect();

                if items.is_empty() {
                    return None;
                }

                Some(Component::ActionList {
                    id: format!("category:{}", label.to_lowercase()),
                    items,
                })
            })
            .collect();

        ScreenModel {
            screen_id: "exchange_mode_selection".into(),
            title: "Exchange Mode".into(),
            subtitle: Some("Choose how to exchange contact cards".into()),
            components,
            actions: vec![],
            progress: None,
            ..Default::default()
        }
    }

    /// Handle a user action. Returns `Selected` if a mode was picked.
    pub fn handle_action(&self, action: &UserAction) -> ModeSelectionResult {
        if let UserAction::ListItemSelected {
            component_id: _,
            item_id,
        } = action
            && let Some(name) = item_id.strip_prefix("mode:")
            && let Some(mode) = parse_mode(name)
        {
            return ModeSelectionResult::Selected(mode);
        }
        ModeSelectionResult::Screen(Box::new(self.screen()))
    }
}

/// Serialize mode name for use in item IDs.
trait SerdeName {
    fn serde_name(self) -> &'static str;
}

impl SerdeName for ExchangeMode {
    fn serde_name(self) -> &'static str {
        match self {
            ExchangeMode::Glance => "glance",
            ExchangeMode::Hover => "hover",
            ExchangeMode::Bump => "bump",
            ExchangeMode::Shake => "shake",
            ExchangeMode::Magic => "magic",
            ExchangeMode::TapTap => "tap_tap",
            ExchangeMode::TapHoverShake => "tap_hover_shake",
            ExchangeMode::Link => "link",
            ExchangeMode::Cable => "cable",
            // Safety: all known variants are listed above; new variants added to
            // vauchi-core must also be added here before shipping.
            _ => panic!("unknown ExchangeMode variant — update serde_name()"),
        }
    }
}

/// Parse a mode from its serde name.
fn parse_mode(name: &str) -> Option<ExchangeMode> {
    match name {
        "glance" => Some(ExchangeMode::Glance),
        "hover" => Some(ExchangeMode::Hover),
        "bump" => Some(ExchangeMode::Bump),
        "shake" => Some(ExchangeMode::Shake),
        "magic" => Some(ExchangeMode::Magic),
        "tap_tap" => Some(ExchangeMode::TapTap),
        "tap_hover_shake" => Some(ExchangeMode::TapHoverShake),
        "link" => Some(ExchangeMode::Link),
        "cable" => Some(ExchangeMode::Cable),
        _ => None,
    }
}

// INLINE_TEST_REQUIRED: Tests access private SerdeName trait, parse_mode(), and CATEGORY_ORDER
#[cfg(test)]
mod tests {
    use super::*;

    fn full_caps() -> DeviceCapabilities {
        DeviceCapabilities {
            has_camera: true,
            has_ble: true,
            has_nfc: true,
            audio: vauchi_core::types::AudioCapability::Full,
            has_accelerometer: true,
            has_internet: true,
            has_usb_port: true,
            ..Default::default()
        }
    }

    fn minimal_caps() -> DeviceCapabilities {
        DeviceCapabilities {
            has_camera: true,
            has_ble: false,
            has_nfc: false,
            audio: vauchi_core::types::AudioCapability::None,
            has_accelerometer: false,
            has_internet: true,
            ..Default::default()
        }
    }

    #[test]
    fn screen_shows_all_nine_modes() {
        let engine = ModeSelectionEngine::new(full_caps());
        let screen = engine.screen();
        assert_eq!(screen.screen_id, "exchange_mode_selection");

        // Count total mode items across all ActionList components
        let mode_count: usize = screen
            .components
            .iter()
            .filter_map(|c| match c {
                Component::ActionList { items, .. } => Some(items.len()),
                _ => None,
            })
            .sum();
        assert_eq!(mode_count, 9, "All 9 modes should be listed");
    }

    #[test]
    fn screen_groups_modes_by_category() {
        let engine = ModeSelectionEngine::new(full_caps());
        let screen = engine.screen();

        let category_ids: Vec<&str> = screen
            .components
            .iter()
            .filter_map(|c| match c {
                Component::ActionList { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(
            category_ids,
            vec![
                "category:quick",
                "category:standard",
                "category:fun",
                "category:remote",
            ]
        );
    }

    #[test]
    fn recommended_mode_has_star_icon() {
        let engine = ModeSelectionEngine::new(full_caps());
        let screen = engine.screen();

        // With full caps, recommended should be Hover
        let hover_item = find_mode_item(&screen, "hover");
        assert!(hover_item.is_some(), "Hover should be in the list");
        assert_eq!(
            hover_item.unwrap().icon.as_deref(),
            Some("star.fill"),
            "Recommended mode should have star icon"
        );
    }

    #[test]
    fn unavailable_modes_show_reason() {
        let engine = ModeSelectionEngine::new(minimal_caps());
        let screen = engine.screen();

        // BLE modes should be unavailable
        let magic = find_mode_item(&screen, "magic").expect("Magic should be listed");
        let reason = magic
            .detail
            .as_ref()
            .expect("Unavailable mode should have a detail reason");
        assert!(
            reason.contains("BLE"),
            "Reason should mention BLE, got: {reason}"
        );
    }

    #[test]
    fn available_modes_have_no_detail() {
        let engine = ModeSelectionEngine::new(full_caps());
        let screen = engine.screen();

        let glance = find_mode_item(&screen, "glance");
        assert!(glance.is_some(), "Glance should be listed");
        assert!(
            glance.unwrap().detail.is_none(),
            "Available mode should have no detail"
        );
    }

    #[test]
    fn selecting_mode_returns_selected() {
        let engine = ModeSelectionEngine::new(full_caps());
        let result = engine.handle_action(&UserAction::ListItemSelected {
            component_id: "category:standard".into(),
            item_id: "mode:hover".into(),
        });
        assert!(
            matches!(result, ModeSelectionResult::Selected(ExchangeMode::Hover)),
            "Should return Selected(Hover)"
        );
    }

    #[test]
    fn selecting_unavailable_mode_still_returns_selected() {
        // Availability enforcement is the engine's job, not mode selection's
        let engine = ModeSelectionEngine::new(minimal_caps());
        let result = engine.handle_action(&UserAction::ListItemSelected {
            component_id: "category:fun".into(),
            item_id: "mode:tap_tap".into(),
        });
        assert!(
            matches!(result, ModeSelectionResult::Selected(ExchangeMode::TapTap)),
            "Should return Selected even for unavailable mode"
        );
    }

    #[test]
    fn unknown_action_returns_screen() {
        let engine = ModeSelectionEngine::new(full_caps());
        let result = engine.handle_action(&UserAction::ActionPressed {
            action_id: "something".into(),
        });
        assert!(
            matches!(result, ModeSelectionResult::Screen(..)),
            "Unknown action should return Screen"
        );
    }

    #[test]
    fn all_modes_have_unique_ids() {
        let engine = ModeSelectionEngine::new(full_caps());
        let screen = engine.screen();
        let ids: Vec<&str> = screen
            .components
            .iter()
            .filter_map(|c| match c {
                Component::ActionList { items, .. } => Some(items),
                _ => None,
            })
            .flatten()
            .map(|item| item.id.as_str())
            .collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "All mode IDs should be unique");
    }

    #[test]
    fn serde_name_roundtrips_all_modes() {
        for &mode in ExchangeMode::all() {
            let name = mode.serde_name();
            let parsed = parse_mode(name);
            assert_eq!(
                parsed,
                Some(mode),
                "serde_name roundtrip failed for {:?}",
                mode
            );
        }
    }

    /// Helper: find a mode item by serde name across all components.
    fn find_mode_item<'a>(screen: &'a ScreenModel, mode_name: &str) -> Option<&'a ActionListItem> {
        let target_id = format!("mode:{}", mode_name);
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
