// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mode selection engine — shows all exchange modes grouped by
//! category with availability and recommendation.
//!
//! Wired into `ExchangeEngine` in Phase 1.2.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;
use vauchi_core::exchange::capability::TransportReadiness;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::{DeviceRequirement, ExchangeMode};
use vauchi_core::exchange::mode_availability::{
    ModeAvailability, check_mode_availability_with_readiness, recommend_mode,
};

/// Engine that displays exchange mode selection.
///
/// One hero action (the last-used mode when it can run here, else Glance,
/// else the capability recommendation) with every other mode behind a
/// single "Other ways to connect" disclosure — de-clutter only, nothing
/// hidden (M2 S3, D2.3, user decision 2026-07-04). When the user picks a
/// mode, returns `ModeSelectionResult::Selected`.
pub struct ModeSelectionEngine {
    capabilities: DeviceCapabilities,
    readiness: TransportReadiness,
    hero: ExchangeMode,
    expanded: bool,
    locale: Locale,
}

/// Result of handling an action in the mode selection engine.
pub enum ModeSelectionResult {
    /// User selected a mode.
    Selected(ExchangeMode),
    /// Screen update (e.g., unknown action).
    Screen(Box<ScreenModel>),
}

/// Approved disclosure order (design D2.3): Glance first-class, then the
/// remaining authenticated modes, then the unauthenticated BLE trio
/// (annotated), NFC last (hardware-gated). The hero is filtered out so it
/// is never listed twice.
const DISCLOSURE_ORDER: &[ExchangeMode] = &[
    ExchangeMode::Glance,
    ExchangeMode::Hover,
    ExchangeMode::TapHoverShake,
    ExchangeMode::Link,
    ExchangeMode::Cable,
    ExchangeMode::Bump,
    ExchangeMode::Shake,
    ExchangeMode::Magic,
    ExchangeMode::TapTap,
];

/// Whether a mode can actually start on this device right now.
fn runnable(
    mode: ExchangeMode,
    capabilities: &DeviceCapabilities,
    readiness: &TransportReadiness,
) -> bool {
    matches!(
        check_mode_availability_with_readiness(mode, capabilities, readiness),
        ModeAvailability::Available | ModeAvailability::Degraded { .. }
    )
}

/// Hero pick (D2.3): last-used when runnable, else Glance (implemented +
/// peer-authenticated), else the capability recommendation.
fn pick_hero(
    last_used: Option<ExchangeMode>,
    capabilities: &DeviceCapabilities,
    readiness: &TransportReadiness,
) -> ExchangeMode {
    if let Some(mode) = last_used
        && runnable(mode, capabilities, readiness)
    {
        return mode;
    }
    if runnable(ExchangeMode::Glance, capabilities, readiness) {
        return ExchangeMode::Glance;
    }
    recommend_mode(capabilities)
}

impl ModeSelectionEngine {
    pub fn new(
        capabilities: DeviceCapabilities,
        readiness: TransportReadiness,
        last_used: Option<ExchangeMode>,
        locale: Locale,
    ) -> Self {
        let hero = pick_hero(last_used, &capabilities, &readiness);
        Self {
            capabilities,
            readiness,
            hero,
            expanded: false,
            locale,
        }
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Build the mode selection screen: the hero action, then either the
    /// collapsed "Other ways to connect" entry or (once expanded) the full
    /// ordered mode list with the hero excluded.
    pub fn screen(&self) -> ScreenModel {
        let mut components: Vec<Component> = vec![Component::ActionList {
            id: "hero".into(),
            items: vec![self.mode_item(self.hero)],
        }];

        if self.expanded {
            components.push(Component::ActionList {
                id: "other_modes".into(),
                items: DISCLOSURE_ORDER
                    .iter()
                    .filter(|m| **m != self.hero)
                    .map(|&mode| self.mode_item(mode))
                    .collect(),
            });
        } else {
            components.push(Component::ActionList {
                id: "more".into(),
                items: vec![ActionListItem {
                    id: "show_other_modes".into(),
                    label: self.t("exchange.picker.other_ways"),
                    icon: Some("more".into()),
                    detail: None,
                    a11y: None,
                    info_key: None,
                }],
            });
        }

        ScreenModel {
            screen_id: "exchange_mode_selection".into(),
            title: self.t("exchange.picker.title"),
            subtitle: Some(self.t("exchange.picker.subtitle")),
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
        let is_recommended = mode == self.hero;
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
                label: self.mode_name(mode),
                icon: Some("lock".into()),
                detail: Some(get_string_with_args(
                    self.locale,
                    "exchange.picker.grant",
                    &[("requirement", &requirement_label(self.locale, *requirement))],
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
        let base_detail = match &availability {
            // Availability reasons are core-computed English today —
            // keying them means a reason enum on the availability type
            // (S4b-2 of 2026-07-03-core-screens-bypass-i18n).
            ModeAvailability::Degraded { reason } | ModeAvailability::Unavailable { reason } => {
                reason.clone()
            }
            _ if is_recommended => get_string_with_args(
                self.locale,
                "exchange.picker.recommended",
                &[("detail", &self.mode_instruction(mode))],
            ),
            _ => self.mode_instruction(mode),
        };
        // Bump/Shake/Magic run unauthenticated BLE today (decorative
        // proximity, no peer verification) — say so on the row until their
        // auth tiers land (2026-06-10 BLE records; D2.3 user decision:
        // annotate, don't hide).
        let detail = if matches!(
            mode,
            ExchangeMode::Bump | ExchangeMode::Shake | ExchangeMode::Magic
        ) {
            Some(get_string_with_args(
                self.locale,
                "exchange.picker.unauthenticated",
                &[("detail", &base_detail)],
            ))
        } else {
            Some(base_detail)
        };
        ActionListItem {
            id: format!("mode:{}", mode.serde_name()),
            label: self.mode_name(mode),
            icon: Some(mode_icon(mode).into()),
            detail,
            a11y: None,
            info_key: None,
        }
    }

    /// Mode display name via `exchange.mode_name.*` — the canonical
    /// product feature names ([`ExchangeMode::display_name`] stays for
    /// logs/CLI, where no locale flows).
    fn mode_name(&self, mode: ExchangeMode) -> String {
        get_string(
            self.locale,
            &format!("exchange.mode_name.{}", mode.serde_name()),
        )
    }

    /// Short "what to do" line via `exchange.mode_instruction.*`.
    fn mode_instruction(&self, mode: ExchangeMode) -> String {
        get_string(
            self.locale,
            &format!("exchange.mode_instruction.{}", mode.serde_name()),
        )
    }

    /// Handle a user action. Returns `Selected` if a mode was picked.
    pub fn handle_action(&mut self, action: &UserAction) -> ModeSelectionResult {
        if let UserAction::ListItemSelected {
            component_id: _,
            item_id,
        } = action
        {
            if item_id == "show_other_modes" {
                self.expanded = true;
                return ModeSelectionResult::Screen(Box::new(self.screen()));
            }
            if let Some(name) = item_id.strip_prefix("mode:")
                && let Some(mode) = parse_mode(name)
            {
                return ModeSelectionResult::Selected(mode);
            }
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
///
/// Live via `screens_exchange::intercept_grant_permission` (the grant-affordance
/// tap handler) + the round-trip test. The `--no-default-features` lib check
/// (`-D warnings`, no `cfg(test)`) doesn't trace the `AppEngine` trait-impl path
/// that reaches it, so it is falsely flagged dead there; `requirement_token`
/// survives because its caller (`mode_item`) sits on the `ExchangeEngine`
/// `dyn WorkflowEngine` path rustc does trace. Annotate to keep that build green.
#[allow(dead_code)]
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
/// Requirement display name via `exchange.requirement.*`, keyed by the
/// same token the grant-row id carries; unknown tokens (a new core
/// variant without a key yet) fall back to the generic "Permission".
fn requirement_label(locale: Locale, req: DeviceRequirement) -> String {
    let token = match requirement_token(req) {
        "unknown" => "permission",
        t => t,
    };
    get_string(locale, &format!("exchange.requirement.{token}"))
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
        let mut engine = ModeSelectionEngine::new(
            full_caps(),
            TransportReadiness::default(),
            None,
            Locale::English,
        );
        engine.expanded = true;
        let screen = engine.screen();
        assert_eq!(screen.screen_id, "exchange_mode_selection");

        // Hero + expanded disclosure together list every mode exactly once.
        let mode_count: usize = screen
            .components
            .iter()
            .filter_map(|c| match c {
                Component::ActionList { items, .. } => {
                    Some(items.iter().filter(|i| i.id.starts_with("mode:")).count())
                }
                _ => None,
            })
            .sum();
        assert_eq!(mode_count, 9, "All 9 modes should be listed");
    }

    #[test]
    fn screen_is_hero_plus_disclosure() {
        // Collapsed: hero + the single "Other ways to connect" entry.
        let mut engine = ModeSelectionEngine::new(
            full_caps(),
            TransportReadiness::default(),
            None,
            Locale::English,
        );
        let list_ids = |screen: &ScreenModel| -> Vec<String> {
            screen
                .components
                .iter()
                .filter_map(|c| match c {
                    Component::ActionList { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(list_ids(&engine.screen()), vec!["hero", "more"]);
        // Expanded: hero + the full ordered list (M2 S3, D2.3).
        engine.expanded = true;
        assert_eq!(list_ids(&engine.screen()), vec!["hero", "other_modes"]);
    }

    // @internal
    #[test]
    fn hero_is_first_in_picker() {
        let engine = ModeSelectionEngine::new(
            full_caps(),
            TransportReadiness::default(),
            None,
            Locale::English,
        );
        let screen = engine.screen();
        // First-run hero (no last-used) is Glance — implemented +
        // peer-authenticated (M2 S3, D2.3 user decision 2026-07-04).
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
            Some("mode:glance"),
            "the first-run hero (Glance) should be the first item in the picker"
        );
    }

    // @internal
    #[test]
    fn last_used_mode_becomes_the_hero() {
        let engine = ModeSelectionEngine::new(
            full_caps(),
            TransportReadiness::default(),
            Some(ExchangeMode::Hover),
            Locale::English,
        );
        let screen = engine.screen();
        let hero_first = screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::ActionList { id, items } if id == "hero" => items.first(),
                _ => None,
            })
            .map(|item| item.id.as_str());
        assert_eq!(hero_first, Some("mode:hover"), "last-used mode is the hero");
    }

    // @internal
    #[test]
    fn hero_mode_is_marked_in_detail() {
        let engine = ModeSelectionEngine::new(
            full_caps(),
            TransportReadiness::default(),
            None,
            Locale::English,
        );
        let screen = engine.screen();

        // First-run hero is Glance.
        let glance_item = find_mode_item(&screen, "glance").expect("Glance should be the hero");
        let detail = glance_item
            .detail
            .as_deref()
            .expect("hero mode has a detail subtitle");
        assert!(
            detail.starts_with("Recommended · "),
            "hero detail should carry the marker, got: {detail}"
        );
        // Expected copy comes from the same bundle the engine reads —
        // instruction wording is the locales repo's contract, not this
        // test's. The key-exists guard keeps the assertion non-tautological
        // (a missing key would echo through both sides identically).
        let glance_instruction = get_string(Locale::English, "exchange.mode_instruction.glance");
        assert_ne!(glance_instruction, "exchange.mode_instruction.glance");
        assert!(
            detail.contains(&glance_instruction),
            "hero detail should still include the instruction, got: {detail}"
        );
        // Recommendation no longer rides on the icon — that's the per-mode glyph.
        assert_eq!(glance_item.icon.as_deref(), Some("qrcode"));
    }

    // @internal
    #[test]
    fn every_mode_has_a_per_mode_icon() {
        let mut engine = ModeSelectionEngine::new(
            full_caps(),
            TransportReadiness::default(),
            None,
            Locale::English,
        );
        engine.expanded = true;
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
        let mut engine = ModeSelectionEngine::new(
            minimal_caps(),
            TransportReadiness::default(),
            None,
            Locale::English,
        );
        engine.expanded = true;
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
        let mut engine = ModeSelectionEngine::new(
            full_caps(),
            TransportReadiness::default(),
            None,
            Locale::English,
        );
        engine.expanded = true;
        let screen = engine.screen();

        // Hover is available with full caps and is not the hero (Glance is),
        // so its detail is the plain instruction (no "Recommended" marker).
        let hover = find_mode_item(&screen, "hover").expect("Hover should be listed");
        let hover_instruction = get_string(Locale::English, "exchange.mode_instruction.hover");
        assert_ne!(hover_instruction, "exchange.mode_instruction.hover");
        assert_eq!(
            hover.detail.as_deref(),
            Some(hover_instruction.as_str()),
            "available non-hero mode shows its instruction"
        );
    }

    #[test]
    fn selecting_mode_returns_selected() {
        let mut engine = ModeSelectionEngine::new(
            full_caps(),
            TransportReadiness::default(),
            None,
            Locale::English,
        );
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
        let mut engine = ModeSelectionEngine::new(
            minimal_caps(),
            TransportReadiness::default(),
            None,
            Locale::English,
        );
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
        let mut engine = ModeSelectionEngine::new(
            full_caps(),
            TransportReadiness::default(),
            None,
            Locale::English,
        );
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
        let engine = ModeSelectionEngine::new(
            full_caps(),
            TransportReadiness::default(),
            None,
            Locale::English,
        );
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
        let mut engine = ModeSelectionEngine::new(full_caps(), led, None, Locale::English);
        engine.expanded = true;
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
        let engine = ModeSelectionEngine::new(full_caps(), led, None, Locale::English);
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
        let mut engine = ModeSelectionEngine::new(full_caps(), led, None, Locale::English);
        engine.expanded = true;
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
