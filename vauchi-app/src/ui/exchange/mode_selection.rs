// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mode selection engine — shows all exchange modes grouped by
//! category with availability and recommendation.
//!
//! Wired into `ExchangeEngine` in Phase 1.2.

use crate::ui::*;
use vauchi_core::exchange::capability::TransportReadiness;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::{DeviceRequirement, ExchangeMode, ModeCategory};
use vauchi_core::exchange::mode_availability::{
    ModeAvailability, check_mode_availability_with_readiness, recommend_mode,
};

/// Engine that displays exchange mode selection.
///
/// Shows all 9 modes grouped by `ModeCategory`, highlights the
/// recommended mode, and grays out unavailable modes with a reason.
/// When the user picks a mode, returns `ModeSelectionResult::Selected`.
pub struct ModeSelectionEngine {
    capabilities: DeviceCapabilities,
    readiness: TransportReadiness,
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
    pub fn new(capabilities: DeviceCapabilities, readiness: TransportReadiness) -> Self {
        let recommended = recommend_mode(&capabilities);
        Self {
            capabilities,
            readiness,
            recommended,
        }
    }

    /// Build the mode selection screen.
    ///
    /// The dynamically-recommended mode (e.g. Hover with full capabilities)
    /// leads the picker in its own `recommended` group so the suggested ritual
    /// comes first; the remaining modes follow grouped by category, with the
    /// recommended mode excluded so it is never listed twice. See
    /// `_private/docs/problems/2026-06-06-exchange-ritual-flow/` (Hover first).
    pub fn screen(&self) -> ScreenModel {
        let mut components: Vec<Component> = vec![Component::ActionList {
            id: "recommended".into(),
            items: vec![self.mode_item(self.recommended)],
        }];

        for (category, label) in CATEGORY_ORDER {
            let items: Vec<ActionListItem> = ExchangeMode::all()
                .iter()
                .filter(|m| m.category() == *category && **m != self.recommended)
                .map(|&mode| self.mode_item(mode))
                .collect();

            if items.is_empty() {
                continue;
            }

            components.push(Component::ActionList {
                id: format!("category:{}", label.to_lowercase()),
                items,
            });
        }

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

    /// Build the list item for a single mode: availability-aware detail,
    /// recommendation marker, and per-mode icon.
    fn mode_item(&self, mode: ExchangeMode) -> ActionListItem {
        let availability =
            check_mode_availability_with_readiness(mode, &self.capabilities, &self.readiness);
        let is_recommended = mode == self.recommended;
        // A present-but-permission-denied transport is recoverable: render the
        // row as a grant affordance with its own id prefix (`grant:<mode>:<req>`,
        // intercepted by AppEngine to re-enable the requirement and rebuild)
        // rather than a `mode:` selection that would enter a wait it can't win.
        // See `_private/docs/problems/2026-06-11-exchange-waits-forever-without-capabilities/`.
        if let ModeAvailability::PermissionRequired { requirement } = &availability {
            return ActionListItem {
                id: format!(
                    "grant:{}:{}",
                    mode.serde_name(),
                    requirement_token(*requirement)
                ),
                label: mode.display_name().to_string(),
                icon: Some("lock".into()),
                detail: Some(format!(
                    "{} permission needed — tap to grant",
                    requirement_label(*requirement)
                )),
                a11y: None,
                info_key: None,
            };
        }
        // When the mode can't run, the availability reason is the most useful
        // subtitle; otherwise show the short "what to do" instruction. The
        // recommended mode (always available, since `recommend_mode` only picks
        // runnable modes) gets a leading marker so the suggestion survives
        // without a dedicated badge field on the wire item.
        let detail = match &availability {
            ModeAvailability::Degraded { reason } | ModeAvailability::Unavailable { reason } => {
                Some(reason.clone())
            }
            _ if is_recommended => Some(format!("Recommended · {}", mode_instruction(mode))),
            _ => Some(mode_instruction(mode).to_string()),
        };
        ActionListItem {
            id: format!("mode:{}", mode.serde_name()),
            label: mode.display_name().to_string(),
            icon: Some(mode_icon(mode).into()),
            detail,
            a11y: None,
            info_key: None,
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

/// Semantic icon token for each mode. Frontends map these names to their
/// native symbol set (Material on Android, SF Symbols on iOS/macOS);
/// unknown tokens fall back to a generic glyph, so the list never breaks.
fn mode_icon(mode: ExchangeMode) -> &'static str {
    match mode {
        ExchangeMode::Glance => "qrcode",
        ExchangeMode::Hover => "nfc",
        ExchangeMode::Bump => "bump",
        ExchangeMode::Shake => "shake",
        ExchangeMode::Magic => "sparkles",
        ExchangeMode::TapTap => "tap",
        ExchangeMode::TapHoverShake => "gesture",
        ExchangeMode::Link => "link",
        ExchangeMode::Cable => "cable",
        // New core variants must add an icon token before shipping.
        _ => "tag",
    }
}

/// One-line "what you do" instruction per mode, shown as the row subtitle.
/// Kept short enough to read on a phone list row.
fn mode_instruction(mode: ExchangeMode) -> &'static str {
    match mode {
        ExchangeMode::Glance => "Point cameras at each other's screen",
        ExchangeMode::Hover => "Hold the phones close together",
        ExchangeMode::Bump => "Gently bump the phones together",
        ExchangeMode::Shake => "Hold together, then shake",
        ExchangeMode::Magic => "Bring the phones close — it connects itself",
        ExchangeMode::TapTap => "Tap the phones together twice",
        ExchangeMode::TapHoverShake => "Tap, hold close, then shake",
        ExchangeMode::Link => "Send a link to connect remotely",
        ExchangeMode::Cable => "Connect both with a USB cable",
        // New core variants must add an instruction before shipping.
        _ => "",
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

/// Stable lowercase token for a requirement, used in grant-affordance item ids
/// (`grant:<mode>:<token>`). Mirrors `DeviceRequirement`'s serde snake_case so
/// the AppEngine can recover the requirement via [`parse_requirement`].
pub(crate) fn requirement_token(req: DeviceRequirement) -> &'static str {
    match req {
        DeviceRequirement::Camera => "camera",
        DeviceRequirement::Ble => "ble",
        DeviceRequirement::Nfc => "nfc",
        DeviceRequirement::Microphone => "microphone",
        DeviceRequirement::Speaker => "speaker",
        DeviceRequirement::Accelerometer => "accelerometer",
        DeviceRequirement::Internet => "internet",
        DeviceRequirement::UsbPort => "usb_port",
        // `DeviceRequirement` is `#[non_exhaustive]`; a new core variant must
        // add a token here before its modes can ship a grant affordance.
        _ => "unknown",
    }
}

/// Inverse of [`requirement_token`]; `None` for an unknown token.
pub(crate) fn parse_requirement(token: &str) -> Option<DeviceRequirement> {
    Some(match token {
        "camera" => DeviceRequirement::Camera,
        "ble" => DeviceRequirement::Ble,
        "nfc" => DeviceRequirement::Nfc,
        "microphone" => DeviceRequirement::Microphone,
        "speaker" => DeviceRequirement::Speaker,
        "accelerometer" => DeviceRequirement::Accelerometer,
        "internet" => DeviceRequirement::Internet,
        "usb_port" => DeviceRequirement::UsbPort,
        _ => return None,
    })
}

/// Human-readable requirement name for the grant-affordance detail line.
fn requirement_label(req: DeviceRequirement) -> &'static str {
    match req {
        DeviceRequirement::Camera => "Camera",
        DeviceRequirement::Ble => "Bluetooth",
        DeviceRequirement::Nfc => "NFC",
        DeviceRequirement::Microphone => "Microphone",
        DeviceRequirement::Speaker => "Speaker",
        DeviceRequirement::Accelerometer => "Motion",
        DeviceRequirement::Internet => "Internet",
        DeviceRequirement::UsbPort => "USB",
        _ => "Permission",
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
        let engine = ModeSelectionEngine::new(full_caps(), TransportReadiness::default());
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
        let engine = ModeSelectionEngine::new(full_caps(), TransportReadiness::default());
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
                "recommended",
                "category:quick",
                "category:standard",
                "category:fun",
                "category:remote",
            ]
        );
    }

    // @internal
    #[test]
    fn recommended_mode_is_first_in_picker() {
        let engine = ModeSelectionEngine::new(full_caps(), TransportReadiness::default());
        let screen = engine.screen();
        // The recommended mode (Hover with full caps) leads the picker as the
        // first item of the first ("recommended") group — see
        // _private/docs/problems/2026-06-06-exchange-ritual-flow/ (Hover first).
        let first_item_id = screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::ActionList { items, .. } => items.first(),
                _ => None,
            })
            .map(|item| item.id.as_str());
        assert_eq!(
            first_item_id,
            Some("mode:hover"),
            "the recommended mode (Hover) should be the first item in the picker"
        );
    }

    // @internal
    #[test]
    fn recommended_mode_is_marked_in_detail() {
        let engine = ModeSelectionEngine::new(full_caps(), TransportReadiness::default());
        let screen = engine.screen();

        // With full caps, recommended should be Hover.
        let hover_item = find_mode_item(&screen, "hover").expect("Hover should be in the list");
        let detail = hover_item
            .detail
            .as_deref()
            .expect("recommended mode has a detail subtitle");
        assert!(
            detail.starts_with("Recommended · "),
            "recommended detail should carry the marker, got: {detail}"
        );
        assert!(
            detail.contains("Hold the phones close together"),
            "recommended detail should still include the instruction, got: {detail}"
        );
        // Recommendation no longer rides on the icon — that's now the per-mode glyph.
        assert_eq!(hover_item.icon.as_deref(), Some("nfc"));
    }

    // @internal
    #[test]
    fn every_mode_has_a_per_mode_icon() {
        let engine = ModeSelectionEngine::new(full_caps(), TransportReadiness::default());
        let screen = engine.screen();
        for &mode in ExchangeMode::all() {
            let item =
                find_mode_item(&screen, mode.serde_name()).expect("every mode should be listed");
            let icon = item.icon.as_deref().expect("every mode carries an icon");
            assert_ne!(icon, "tag", "{:?} should have a dedicated icon", mode);
            assert!(!icon.is_empty(), "{:?} icon must be non-empty", mode);
        }
    }

    #[test]
    fn unavailable_modes_show_reason() {
        let engine = ModeSelectionEngine::new(minimal_caps(), TransportReadiness::default());
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

    // @internal
    #[test]
    fn available_modes_show_instruction_detail() {
        let engine = ModeSelectionEngine::new(full_caps(), TransportReadiness::default());
        let screen = engine.screen();

        // Glance is available with full caps and is not the recommended mode,
        // so its detail is the plain instruction (no "Recommended" marker).
        let glance = find_mode_item(&screen, "glance").expect("Glance should be listed");
        assert_eq!(
            glance.detail.as_deref(),
            Some("Point cameras at each other's screen"),
            "available non-recommended mode shows its instruction"
        );
    }

    #[test]
    fn selecting_mode_returns_selected() {
        let engine = ModeSelectionEngine::new(full_caps(), TransportReadiness::default());
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
        let engine = ModeSelectionEngine::new(minimal_caps(), TransportReadiness::default());
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
        let engine = ModeSelectionEngine::new(full_caps(), TransportReadiness::default());
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
        let engine = ModeSelectionEngine::new(full_caps(), TransportReadiness::default());
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

    // @internal
    #[test]
    fn permission_denied_present_mode_renders_grant_affordance() {
        // Glance requires Camera + Ble; both present on a full device, but the
        // camera permission is denied → the row becomes a grant affordance
        // (`grant:glance:camera`), not a selectable `mode:glance`.
        let mut led = TransportReadiness::default();
        led.note_denied(DeviceRequirement::Camera);
        let engine = ModeSelectionEngine::new(full_caps(), led);
        let screen = engine.screen();

        let grant = find_item_starting_with(&screen, "grant:glance:")
            .expect("denied Glance should render a grant affordance");
        assert_eq!(grant.id, "grant:glance:camera");
        assert_eq!(grant.icon.as_deref(), Some("lock"));
        let detail = grant
            .detail
            .as_deref()
            .expect("grant affordance carries a detail line");
        assert!(
            detail.contains("Camera"),
            "detail should name the requirement, got: {detail}"
        );
        assert!(
            find_mode_item(&screen, "glance").is_none(),
            "a denied mode must not also appear as a selectable mode:glance row"
        );
    }

    // @internal
    #[test]
    fn granting_restores_selectable_mode_row() {
        // Last-write-wins: deny then grant Camera → the render reverts to a
        // normal selectable `mode:glance` row with no grant affordance. (The
        // AppEngine rebuilds the engine with the updated ledger; this asserts
        // the render is permission-aware.)
        let mut led = TransportReadiness::default();
        led.note_denied(DeviceRequirement::Camera);
        led.note_granted(DeviceRequirement::Camera);
        let engine = ModeSelectionEngine::new(full_caps(), led);
        let screen = engine.screen();

        assert!(
            find_mode_item(&screen, "glance").is_some(),
            "granted camera should restore the selectable mode:glance row"
        );
        assert!(
            find_item_starting_with(&screen, "grant:glance:").is_none(),
            "a granted mode must not render a grant affordance"
        );
    }

    // @internal
    #[test]
    fn multiple_denied_requirements_yield_one_affordance_for_the_first() {
        // User-confirmed design (2026-06-14): one grant affordance per mode,
        // targeting its FIRST denied requirement. Glance requires Camera + BLE;
        // deny both → exactly one `grant:glance:*` row. Granting the first
        // re-renders and surfaces the next (progressive disclosure), so the
        // per-step detail always names the real current blocker.
        let mut led = TransportReadiness::default();
        led.note_denied(DeviceRequirement::Camera);
        led.note_denied(DeviceRequirement::Ble);
        let engine = ModeSelectionEngine::new(full_caps(), led);
        let screen = engine.screen();

        let grants: Vec<String> = screen
            .components
            .iter()
            .filter_map(|c| match c {
                Component::ActionList { items, .. } => Some(items),
                _ => None,
            })
            .flatten()
            .filter(|i| i.id.starts_with("grant:glance:"))
            .map(|i| i.id.clone())
            .collect();
        assert_eq!(
            grants.len(),
            1,
            "exactly one grant affordance per mode (its first denied req), got {grants:?}"
        );
        assert!(
            find_mode_item(&screen, "glance").is_none(),
            "a doubly-denied Glance must not also be a selectable mode"
        );
    }

    // @internal
    #[test]
    fn requirement_token_roundtrips_all_variants() {
        for req in [
            DeviceRequirement::Camera,
            DeviceRequirement::Ble,
            DeviceRequirement::Nfc,
            DeviceRequirement::Microphone,
            DeviceRequirement::Speaker,
            DeviceRequirement::Accelerometer,
            DeviceRequirement::Internet,
            DeviceRequirement::UsbPort,
        ] {
            assert_eq!(
                parse_requirement(requirement_token(req)),
                Some(req),
                "token roundtrip failed for {req:?}"
            );
        }
        assert_eq!(parse_requirement("nonsense"), None);
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

    /// Helper: find the first item whose id starts with `prefix`.
    fn find_item_starting_with<'a>(
        screen: &'a ScreenModel,
        prefix: &str,
    ) -> Option<&'a ActionListItem> {
        screen
            .components
            .iter()
            .filter_map(|c| match c {
                Component::ActionList { items, .. } => Some(items),
                _ => None,
            })
            .flatten()
            .find(|item| item.id.starts_with(prefix))
    }
}
