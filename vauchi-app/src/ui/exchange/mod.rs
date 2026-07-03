// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange engine — QR-based contact exchange workflow.
//!
//! ADR-031: ExchangeEngine holds an optional `ExchangeSession` to connect
//! the UI workflow with the cryptographic protocol state machine. When a
//! session is provided, transitions emit `Command`s that frontends
//! dispatch to platform hardware (camera, BLE, NFC, audio).

use std::sync::Arc;

mod back_nav;
pub(crate) mod ble;
pub mod ble_engine;
pub mod direct_engine;
pub(crate) mod field_preview;
pub(crate) mod group_filter;
pub(crate) mod mode_selection;
pub(crate) mod nfc;
pub mod nfc_engine;
pub(crate) mod scan_quality;
pub(crate) mod success;
pub(crate) mod verifying;

use self::field_preview::{FieldPreviewConfig, FieldPreviewResult};
use self::mode_selection::{ModeSelectionEngine, ModeSelectionResult};
use self::nfc::NfcStep;
use crate::ui::*;
use vauchi_core::Command;
use vauchi_core::clock::Clock;
use vauchi_core::exchange::capability::TransportReadiness;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::{DeviceRequirement, ExchangeMode};
use vauchi_core::exchange::mode_availability::{
    ModeAvailability, check_mode_availability_with_readiness,
};
use vauchi_core::exchange::{ExchangeEvent, ExchangeSession};

use crate::ui::reciprocity_confirmer::ReciprocityConfirmer;

/// Configuration for starting an exchange.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExchangeConfig {
    pub own_name: String,
    pub own_qr_data: String,
    /// Available groups for pre-selection (id, name). Empty = no group picker.
    #[serde(default)]
    pub available_groups: Vec<(String, String)>,
    /// Device hardware capabilities for mode availability checking.
    #[serde(default)]
    #[cfg_attr(feature = "schema-gen", schemars(skip))]
    pub device_capabilities: DeviceCapabilities,
    /// Runtime OS-permission ledger (capability × permission). Transient: the
    /// factory rebuilds it from the live AppEngine field on every build, so it
    /// is never serialized (the wire form carries no permission state). The
    /// mode picker consults it to render grant affordances for denied-but-
    /// present transports. See
    /// `2026-06-11-exchange-waits-forever-without-capabilities`.
    #[serde(skip)]
    #[cfg_attr(feature = "schema-gen", schemars(skip))]
    pub transport_readiness: TransportReadiness,
    /// Selected exchange mode. `None` = show mode selection first.
    #[serde(default)]
    pub mode: Option<ExchangeMode>,
    /// Frozen card snapshot for exchange. `None` = snapshot at exchange start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema-gen", schemars(skip))]
    pub card_snapshot: Option<vauchi_core::exchange::card_snapshot::CardSnapshot>,
    /// Owner's groups (with their `visible_fields`), used to resolve which
    /// fields the selected exchange group(s) may see — fed to
    /// [`group_filter::resolve_exchange_allow`] so the field preview matches
    /// the card the BLE payload actually transmits. Empty = no group data
    /// (share all). See `2026-06-08-exchange-card-not-group-filtered`.
    #[serde(default)]
    #[cfg_attr(feature = "schema-gen", schemars(skip))]
    pub available_group_data: Vec<vauchi_core::Group>,
}

/// Engine that drives the QR exchange workflow.
///
/// ADR-031: When `session` is `Some`, the engine delegates protocol state
/// transitions to `ExchangeSession` and emits `Command`s via
/// `ActionResult::Commands`. When `session` is `None`, the engine
/// behaves as a UI-only workflow (legacy behavior).
pub struct ExchangeEngine {
    step: ExchangeStep,
    /// Back-stack of *selection-phase* steps the user advanced through
    /// (mode → group → field-preview → sub-flow entry). Drives the
    /// engine-internal BACK (`navigate_back_within`) so a press rewinds
    /// one step instead of tearing down the whole Exchange screen. Only
    /// pushed on user-initiated forward transitions; protocol-driven
    /// transitions (handshake progress, success/failure) never push.
    step_history: Vec<ExchangeStep>,
    config: ExchangeConfig,
    scanned_data: Option<String>,
    /// Groups selected by the user before exchange.
    selected_groups: Vec<String>,
    /// G4: whether to show the field preview after mode selection —
    /// set by the group step (`continue` = yes, `skip` = no), read
    /// after mode is chosen (group-first ordering).
    show_preview_after_mode: bool,
    /// ADR-031: Protocol session for hardware command/event exchange.
    session: Option<ExchangeSession>,
    /// User-friendly error detail shown on the Failed screen (T1-2).
    failure_detail: Option<String>,
    /// Whether relay fallback is available on the Failed screen (BLE mode failures).
    ble_fallback_available: bool,
    /// Whether QR fallback is available on the Failed screen (BLE mode failures with camera).
    qr_fallback_available: bool,
    /// Mode selection sub-engine (created on demand).
    mode_selection: Option<ModeSelectionEngine>,
    /// Field preview config (built when entering FieldPreview step).
    field_preview: Option<FieldPreviewConfig>,
    /// Reciprocity confirmation cascade driver (created on exchange completion).
    reciprocity_confirmer: Option<ReciprocityConfirmer>,
    /// Terminal reciprocity outcome, once the confirmer resolves. Latched so
    /// the `Complete` step-sync does not re-arm a fresh confirmer — session
    /// confirmation tokens persist through `Complete`, so without this the
    /// re-create guard fires forever and the flow never leaves `Verifying`.
    reciprocity_outcome: Option<vauchi_core::exchange::reciprocity::Reciprocity>,
    /// Wall-clock source for time-stamped sub-engines
    /// (`ReciprocityConfirmer`). Threaded through both constructors;
    /// production callers pass `vauchi.clock()`, tests use
    /// `SystemClock::shared()` or a `FakeClock`.
    clock: Arc<dyn Clock>,
    /// Rich success-screen content built from the session's completed
    /// contact (+ the field-preview the user shared) when the exchange
    /// reaches `Success`. `None` → minimal completion chrome. Mirrors the
    /// multi-stage / link engines so every mode renders the shared screen.
    success_summary: Option<crate::ui::exchange::success::ExchangeSuccessSummary>,
}

#[derive(Clone, Debug, PartialEq)]
enum ExchangeStep {
    /// User picks an exchange mode (first step when mode is not pre-set).
    ModeSelection,
    /// Pick groups for the new contact (shown only if groups exist).
    GroupSelection,
    /// Read-only preview of what will be shared (after group selection).
    FieldPreview,
    /// Flow-agnostic "verification in progress". Set by the session->step
    /// sync once the protocol passes display/scan; rendered by
    /// `verifying::build_verifying_screen`. Replaces the legacy
    /// `Qr(QrStep::Verifying)`.
    Verifying,
    Success,
    Failed,
}

impl ExchangeStep {
    fn step_number(&self) -> u8 {
        match self {
            Self::ModeSelection => 1,
            Self::GroupSelection => 2,
            Self::FieldPreview => 3,
            Self::Verifying => 6,
            Self::Success => 4 + NfcStep::STEP_COUNT,
            Self::Failed => 5 + NfcStep::STEP_COUNT,
        }
    }
}

// mode + group + preview + sub-flow + success/failed
const TOTAL_STEPS: u8 = 3 + NfcStep::STEP_COUNT + 2;

impl ExchangeEngine {
    /// Determine the initial step based on config.
    ///
    /// If mode is pre-set, skip mode selection (backward compat / tests).
    /// Otherwise start at ModeSelection.
    fn initial_step(config: &ExchangeConfig) -> ExchangeStep {
        // G4 (group-first): when groups exist the exchange begins with group
        // selection, then mode (a shared ritual chosen together). No groups →
        // straight to mode selection.
        if config.available_groups.is_empty() {
            ExchangeStep::ModeSelection
        } else {
            ExchangeStep::GroupSelection
        }
    }

    pub fn new(config: ExchangeConfig, clock: Arc<dyn Clock>) -> Self {
        let step = Self::initial_step(&config);
        let mode_selection = if step == ExchangeStep::ModeSelection {
            Some(ModeSelectionEngine::new(
                config.device_capabilities.clone(),
                config.transport_readiness.clone(),
            ))
        } else {
            None
        };
        Self {
            step,
            step_history: Vec::new(),
            config,
            scanned_data: None,
            selected_groups: Vec::new(),
            show_preview_after_mode: false,
            session: None,
            failure_detail: None,
            ble_fallback_available: false,
            qr_fallback_available: false,
            mode_selection,
            field_preview: None,
            reciprocity_confirmer: None,
            reciprocity_outcome: None,
            clock,
            success_summary: None,
        }
    }

    /// Creates a new ExchangeEngine with a protocol session (ADR-031).
    ///
    /// When a session is provided, the engine emits `Command`s
    /// at each step transition, connecting the UI workflow with the
    /// cryptographic protocol state machine.
    ///
    /// If no group selection is needed, the session is started immediately
    /// (StartQR applied). Use `drain_commands()` to get the initial
    /// `QrDisplay` command after construction.
    pub fn with_session(
        config: ExchangeConfig,
        mut session: ExchangeSession,
        clock: Arc<dyn Clock>,
    ) -> Self {
        // Always enable debug logging — negligible overhead, data only
        // consumed when explicitly requested via exchange_debug_log().
        session.enable_debug_log();

        let step = Self::initial_step(&config);
        let mode_selection = if step == ExchangeStep::ModeSelection {
            Some(ModeSelectionEngine::new(
                config.device_capabilities.clone(),
                config.transport_readiness.clone(),
            ))
        } else {
            None
        };

        Self {
            step,
            step_history: Vec::new(),
            config,
            scanned_data: None,
            selected_groups: Vec::new(),
            show_preview_after_mode: false,
            session: Some(session),
            failure_detail: None,
            ble_fallback_available: false,
            qr_fallback_available: false,
            mode_selection,
            field_preview: None,
            reciprocity_confirmer: None,
            reciprocity_outcome: None,
            clock,
            success_summary: None,
        }
    }

    /// Drains any pending commands from the session or Link mode (ADR-031).
    ///
    /// Call this after construction with `with_session()` to get the
    /// initial `QrDisplay` command, or after `new()` with Link mode to get
    /// the initial presence deposit command.
    pub fn drain_commands(&mut self) -> Vec<Command> {
        self.session
            .as_mut()
            .map(|s| s.drain_commands())
            .unwrap_or_default()
    }

    /// Returns a reference to the protocol session, if any (ADR-031).
    pub fn session(&self) -> Option<&ExchangeSession> {
        self.session.as_ref()
    }

    /// Returns a mutable reference to the protocol session, if any (ADR-031).
    pub fn session_mut(&mut self) -> Option<&mut ExchangeSession> {
        self.session.as_mut()
    }

    /// Returns the confirmation state for persistence (crash recovery).
    ///
    /// Available after exchange completion. The platform layer should
    /// encrypt and save this to the contact's `confirmation_state` column.
    pub fn confirmation_state(
        &self,
    ) -> Option<crate::ui::reciprocity_confirmer::ConfirmationState> {
        self.reciprocity_confirmer
            .as_ref()
            .map(|c| c.to_persisted_state())
    }

    pub fn selected_groups(&self) -> &[String] {
        &self.selected_groups
    }

    /// Mark the exchange as successfully verified.
    pub fn mark_success(&mut self) {
        self.step = ExchangeStep::Success;
    }

    pub fn mark_failed(&mut self) {
        self.step = ExchangeStep::Failed;
    }

    /// Mark the exchange as failed with a specific error detail for the user.
    pub fn mark_failed_with_error(&mut self, error: &vauchi_core::exchange::ExchangeError) {
        self.failure_detail = Some(error.user_message().to_string());
        self.step = ExchangeStep::Failed;
    }

    /// Plain-language grant prompt for a denied required transport, shown on the
    /// T2.3 fail-fast retry/cancel screen. Only the permission-bearing
    /// requirements (camera/ble/nfc/microphone — the ones a `PermissionDenied`
    /// event maps to) can reach here; presence-only requirements never yield
    /// `PermissionRequired`, so the catch-all arm is unreachable in practice.
    fn permission_denied_detail(req: DeviceRequirement) -> String {
        let what = match req {
            DeviceRequirement::Camera => "Camera access",
            DeviceRequirement::Ble => "Bluetooth access",
            DeviceRequirement::Nfc => "NFC access",
            DeviceRequirement::Microphone => "Microphone access",
            // `DeviceRequirement` is `#[non_exhaustive]`, so this arm is
            // required. It is unreachable today (only the four
            // permission-bearing requirements can be `PermissionRequired`); the
            // assert fails loudly in tests if a new permission-bearing
            // requirement is added without a grant prompt here.
            other => {
                debug_assert!(
                    false,
                    "permission_denied_detail: no grant prompt for {other:?}"
                );
                "A required permission"
            }
        };
        format!("{what} was denied. Grant it in Settings, then retry.")
    }

    pub fn scanned_data(&self) -> Option<&str> {
        self.scanned_data.as_deref()
    }

    /// Start the protocol session (ADR-031) when entering ShowQr.
    ///
    /// Hand off to the graduated `LinkExchangeEngine` (ADR-021/043 Humble
    /// UI). The link-mode initiator flow (share-url / waiting / retrieving /
    /// terminal screens) now lives in its own engine driven by the
    /// engine-owned `LinkInitiatorSession`; this engine never enters a Link
    /// sub-flow. AppEngine routes `StartLinkExchange` to construct the new
    /// engine and build its `LinkInitiatorSession`.
    fn start_link_mode(&self) -> ActionResult {
        ActionResult::StartLinkExchange
    }

    /// Route to the mode-specific sub-flow once the optional group /
    /// field-preview steps are done. Single source of truth for
    /// `ExchangeMode` → `ExchangeStep`, so the three entry points
    /// (no-groups direct, group-Skip, field-preview start) cannot
    /// diverge — that divergence silently routed grouped TapTap, Glance
    /// and Hover through the legacy QR step
    /// (`2026-06-02-grouped-mode-routing-nfc`). Relies on
    /// `self.config.mode`, set when the mode is picked (before any group
    /// detour).
    fn enter_mode_sub_flow(&mut self) -> ActionResult {
        // T2.3: fail fast before entering a wait a denied OS permission makes
        // unwinnable. A present-but-denied required transport routes to the
        // existing retry/cancel failure screen instead of the silent wait that
        // the device-stress run observed. Reuses the T2.2 picker helper;
        // Internet/UsbPort (presence-only, never `PermissionDenied`) never match,
        // so Link/Cable fall through unchanged. The mid-session case (denial
        // *during* a live wait) is already handled by each wait engine's own
        // `PermissionDenied` branch.
        //
        // Scope: this guards the *denied-permission* case only
        // (`PermissionRequired`). A `ModeAvailability::Unavailable`
        // (hardware-absent) mode is NOT caught here — it has no grant path, the
        // picker already filters it, and if one still reaches entry (a caps
        // hot-plug race) the wait engine's own deadline (Phase 1) backstops the
        // forever-wait. Extending fail-fast to absent hardware is out of scope.
        // See `2026-06-11-exchange-waits-forever-without-capabilities` (T2.3).
        if let Some(mode) = self.config.mode
            && let ModeAvailability::PermissionRequired { requirement } =
                check_mode_availability_with_readiness(
                    mode,
                    &self.config.device_capabilities,
                    &self.config.transport_readiness,
                )
        {
            self.failure_detail = Some(Self::permission_denied_detail(requirement));
            self.step = ExchangeStep::Failed;
            return ActionResult::NavigateTo(self.build_screen());
        }
        match self.config.mode {
            Some(ExchangeMode::Link) => self.start_link_mode(),
            Some(
                mode @ (ExchangeMode::Magic
                | ExchangeMode::Bump
                | ExchangeMode::Shake
                | ExchangeMode::Glance),
            ) => {
                // The BLE flow runs in its own `AppScreen::BleExchange` engine
                // (`BleExchangeEngine`); this cached `ExchangeEngine` is kept for
                // Cancel. Re-arm the picker so it is not a zombie (Fix A of
                // 2026-06-02-exchange-back-cancel-broken).
                // G3: Glance joins the BLE family — BLE data transport,
                // bootstrapped by a one-sided QR scan, with no proximity signal
                // (the engine runs no proximity runner for Glance).
                self.mode_selection = Some(ModeSelectionEngine::new(
                    self.config.device_capabilities.clone(),
                    self.config.transport_readiness.clone(),
                ));
                ActionResult::StartBleExchange { mode }
            }
            Some(ExchangeMode::TapTap) => {
                // The NFC flow (Send/Receive role choice + 3-phase tap
                // handshake) runs in its own `AppScreen::NfcExchange` engine
                // (`NfcExchangeEngine`); this cached `ExchangeEngine` is kept
                // for Cancel. Re-arm the picker so it is not a zombie (Fix A of
                // 2026-06-02-exchange-back-cancel-broken).
                self.mode_selection = Some(ModeSelectionEngine::new(
                    self.config.device_capabilities.clone(),
                    self.config.transport_readiness.clone(),
                ));
                ActionResult::StartNfcExchange
            }
            // Hover / TapHoverShake are the multi-stage-QR modes —
            // bidirectional simultaneous QR + a proximity signal (Hover:
            // ultrasonic audio; TapHoverShake: audio + accelerometer). They
            // route through the core-driven `MultiStageExchange` screen so the
            // multi-stage protocol drives both QR display and scan from a pure
            // ScreenModel. The `mode` payload tells AppEngine which engine
            // constructor to use (`new_hover` vs `new_tap_hover_shake`). (G3
            // moved `Glance` out of this family to BLE + one-sided QR; the
            // multi-stage-QR transport is Hover's, not Glance's.)
            Some(mode @ (ExchangeMode::Hover | ExchangeMode::TapHoverShake)) => {
                // The multi-stage flow runs in its own
                // `AppScreen::MultiStageExchange` engine, but this
                // `ExchangeEngine` is cached — Cancel navigates back to it.
                // Re-arm the picker so the cached engine is not a zombie: a
                // `ModeSelection` step with `mode_selection == None` renders
                // `ScreenModel::default()` (empty `screen_id` → white
                // screen) and ignores further picks. Fix A of
                // `2026-06-02-exchange-back-cancel-broken`.
                self.mode_selection = Some(ModeSelectionEngine::new(
                    self.config.device_capabilities.clone(),
                    self.config.transport_readiness.clone(),
                ));
                ActionResult::StartMultiStageExchange { mode }
            }
            Some(ExchangeMode::Cable) => {
                // The Cable flow (USB / direct-TCP) runs in its own
                // `AppScreen::DirectTransport` engine (`DirectTransportEngine`);
                // this cached `ExchangeEngine` is kept for Cancel. Re-arm the
                // picker so it is not a zombie (Fix A of
                // 2026-06-02-exchange-back-cancel-broken).
                self.mode_selection = Some(ModeSelectionEngine::new(
                    self.config.device_capabilities.clone(),
                    self.config.transport_readiness.clone(),
                ));
                ActionResult::StartDirectTransport
            }
            // No mode selected — return to the picker (production always
            // enters with mode: None; no mode reaches the retired legacy QR).
            _ => {
                self.mode_selection = Some(ModeSelectionEngine::new(
                    self.config.device_capabilities.clone(),
                    self.config.transport_readiness.clone(),
                ));
                self.step = ExchangeStep::ModeSelection;
                ActionResult::NavigateTo(self.build_screen())
            }
        }
    }

    /// Build a FieldPreviewConfig from the current state.
    ///
    /// Uses own_name as display name (group override would come from
    /// storage, which this engine doesn't have access to yet — deferred
    /// to Phase 2 full integration when AppEngine passes group data).
    fn build_field_preview_config(&self) -> FieldPreviewConfig {
        use vauchi_core::contact_card::ContactCard;
        // Build a preview card from config data
        let card = self
            .config
            .card_snapshot
            .as_ref()
            .map(|s| s.card().clone())
            .unwrap_or_else(|| ContactCard::new(&self.config.own_name));
        // G2: resolve the fields the selected group(s) expose so the preview
        // matches the card the BLE payload transmits. `None` (no groups) →
        // share all; `Some(set)` → share exactly `set` (empty set = share
        // nothing, default-closed). One resolver shared with the BLE payload
        // and the success summary, so the three cannot diverge.
        let visible_field_ids = group_filter::resolve_exchange_allow(
            &self.selected_groups,
            &self.config.available_group_data,
            card.field_visibility(),
        );
        FieldPreviewConfig {
            card,
            display_name: self.config.own_name.clone(),
            visible_field_ids,
        }
    }

    fn progress(&self) -> Progress {
        Progress {
            current_step: self.step.step_number(),
            total_steps: TOTAL_STEPS,
            label: None,
        }
    }

    fn build_screen(&self) -> ScreenModel {
        match self.step {
            ExchangeStep::ModeSelection => {
                if let Some(ref ms) = self.mode_selection {
                    ms.screen()
                } else {
                    // Shouldn't happen — mode_selection is always Some when step is ModeSelection
                    ScreenModel::default()
                }
            }
            ExchangeStep::GroupSelection => build_group_selection_screen(
                &self.config.available_groups,
                &self.selected_groups,
                self.progress(),
            ),
            ExchangeStep::FieldPreview => {
                if let Some(ref fp) = self.field_preview {
                    field_preview::build_field_preview_screen(fp, self.progress())
                } else {
                    ScreenModel::default()
                }
            }
            ExchangeStep::Verifying => verifying::build_verifying_screen(self.progress()),
            ExchangeStep::Success if self.success_summary.is_some() => {
                let summary = self.success_summary.as_ref().expect("guarded by is_some()");
                let mut screen = crate::ui::exchange::success::build_exchange_success_screen(
                    "exchange_success",
                    "Success",
                    "done",
                    summary,
                );
                screen.progress = Some(self.progress());
                screen
            }
            ExchangeStep::Success => ScreenModel {
                screen_id: "exchange_success".into(),
                title: "Success".into(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "success_status".into(),
                    icon: None,
                    title: "Exchange Complete".into(),
                    detail: None,
                    status: Status::Success,
                    a11y: Some(A11y {
                        label: Some("Exchange complete".into()),
                        hint: Some("Contact cards have been exchanged successfully".into()),
                        role: None,
                    }),
                }],
                actions: vec![ScreenAction {
                    id: "done".into(),
                    label: "Done".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                }],
                progress: Some(self.progress()),
                ..Default::default()
            },
            ExchangeStep::Failed => self.build_failed_screen(),
        }
    }

    fn build_failed_screen(&self) -> ScreenModel {
        let mut actions = vec![ScreenAction {
            id: "retry".into(),
            label: "Retry".into(),
            style: ActionStyle::Primary,
            enabled: true,
            a11y: None,
        }];
        // BLE failures offer QR and relay fallbacks
        if self.qr_fallback_available {
            actions.push(ScreenAction {
                id: "fallback_qr".into(),
                label: "Switch to QR".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                // d4-a11y (selective): "Switch to QR" names a
                // transport whose consequence isn't self-evident —
                // add a hint. label/role stay None (the visible
                // label is the accessible name; no redundant role).
                a11y: Some(A11y {
                    label: None,
                    hint: Some(
                        "Abandons this attempt and restarts the exchange using camera QR codes."
                            .into(),
                    ),
                    role: None,
                }),
            });
        }
        if self.ble_fallback_available {
            actions.push(ScreenAction {
                        id: "fallback_relay".into(),
                        label: "Switch to encrypted relay".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: Some(A11y {
                            label: None,
                            hint: Some(
                                "Abandons this attempt and completes the exchange over the encrypted relay server."
                                    .into(),
                            ),
                            role: None,
                        }),
                    });
        }
        actions.push(ScreenAction {
            id: "cancel".into(),
            label: "Cancel".into(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        });
        ScreenModel {
            screen_id: "exchange_failed".into(),
            title: "Failed".into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "failed_status".into(),
                icon: None,
                title: "Exchange Failed".into(),
                detail: self.failure_detail.clone(),
                status: Status::Failed,
                a11y: Some(A11y {
                    label: Some("Exchange failed".into()),
                    hint: Some("The exchange did not complete. Retry or cancel.".into()),
                    role: None,
                }),
            }],
            actions,
            progress: Some(self.progress()),
            ..Default::default()
        }
    }
}

/// Builds the group selection screen for exchange pre-selection.
///
/// Extracted as a standalone function so it can be reused by
/// the mode-aware exchange flow (field preview also shows groups).
fn build_group_selection_screen(
    available_groups: &[(String, String)],
    selected_groups: &[String],
    progress: Progress,
) -> ScreenModel {
    let items: Vec<ToggleItem> = available_groups
        .iter()
        .map(|(id, name)| ToggleItem {
            id: id.clone(),
            label: name.clone(),
            selected: selected_groups.contains(id),
            subtitle: None,
            // Mirror backup_recovery's ToggleItem a11y (the established
            // convention for toggle leaves): the visible label is the
            // group name, so the a11y label spells out the control's
            // purpose and the role marks it as a toggle. The ToggleList
            // container and the ScreenActions below intentionally stay
            // `None` — a button's `label` is its accessible name, and no
            // ScreenAction in the codebase carries a11y (d4-a11y phase 2).
            a11y: Some(A11y {
                label: Some(format!("{name} group toggle")),
                hint: Some("Toggle to assign the new contact to this group.".into()),
                role: Some(AccessibilityRole::Toggle),
            }),
            info_key: None,
        })
        .collect();
    ScreenModel {
        screen_id: "exchange_group_selection".into(),
        title: "Assign to Groups".into(),
        subtitle: Some("Choose which groups the new contact will be in".into()),
        components: vec![Component::ToggleList {
            id: "group_picker".into(),
            label: "Groups".into(),
            items,
            a11y: None,
        }],
        actions: vec![
            ScreenAction {
                id: "continue".into(),
                label: "Continue".into(),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            },
            ScreenAction {
                id: "skip".into(),
                label: "Skip".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            },
        ],
        progress: Some(progress),
        ..Default::default()
    }
}

impl WorkflowEngine for ExchangeEngine {
    fn can_navigate_back_within(&self) -> bool {
        self.can_back_within()
    }

    fn navigate_back_within(&mut self) -> bool {
        self.back_within()
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_hardware_event(&mut self, event: vauchi_core::Event) -> Option<ActionResult> {
        // No session — handle QR scan via legacy TextChanged path
        let session = match self.session.as_mut() {
            Some(s) => s,
            None => {
                if let vauchi_core::Event::QrScanned { data } = event {
                    let result = self.handle_action(UserAction::TextChanged {
                        component_id: "scanned_data".into(),
                        value: data,
                    });
                    return Some(result);
                }
                return None;
            }
        };
        // Clone event for confirmer routing (event is consumed by session)
        let event_for_confirmer = if self.reciprocity_confirmer.is_some() {
            Some(event.clone())
        } else {
            None
        };
        if let Err(e) = session.apply_hardware_event(event) {
            // The session rejected the event (invalid state, malformed QR, etc.).
            // Transition to Failed so the UI reflects the error.
            self.failure_detail = Some(e.user_message().to_string());
            self.step = ExchangeStep::Failed;
            return Some(ActionResult::UpdateScreen(self.build_screen()));
        }
        let mut commands = session.drain_commands();

        // QR auto-advance: after scanning their QR (PeerScanned), drive the
        // session through the remaining steps to Complete. This mirrors the
        // sequential calls in MobileExchangeSession.processScannedQr() but
        // keeps the logic in core per ADR-031.
        if matches!(
            session.state(),
            vauchi_core::exchange::ExchangeState::PeerScanned { .. }
        ) && session.transport() == vauchi_core::exchange::ExchangeTransport::Qr
        {
            if let Err(e) = session
                .apply(ExchangeEvent::TheyScannedOurQR)
                .and_then(|()| {
                    // Set proximity confidence directly instead of calling
                    // run_proximity_check() — the ManualConfirmationVerifier
                    // starts unconfirmed and would yield Low. Medium is correct
                    // for manual QR exchange (user visually confirmed in-person).
                    session.apply(ExchangeEvent::ProximityCheckCompleted {
                        confidence: vauchi_core::exchange::ProximityConfidence::Medium,
                    })
                })
                .and_then(|()| session.apply(ExchangeEvent::PerformKeyAgreement))
                .and_then(|()| {
                    let our_card = self
                        .config
                        .card_snapshot
                        .as_ref()
                        .map(|s| s.card().clone())
                        .unwrap_or_else(|| {
                            vauchi_core::contact_card::ContactCard::new(&self.config.own_name)
                        });
                    session.apply(ExchangeEvent::CompleteExchange(our_card))
                })
            {
                self.failure_detail = Some(e.user_message().to_string());
                self.step = ExchangeStep::Failed;
                return Some(ActionResult::UpdateScreen(self.build_screen()));
            }
            commands.extend(session.drain_commands());
        }

        // Route escrow events to reciprocity confirmer if active.
        // Check reciprocity result before clearing — the step sync below
        // uses it to decide Success vs Failed.
        if let Some(ref mut confirmer) = self.reciprocity_confirmer {
            if let Some(ref evt) = event_for_confirmer {
                let cmds = confirmer.handle_event(evt);
                commands.extend(cmds);
            }
            if confirmer.is_done() {
                // Latch the terminal outcome so the `Complete` arm below does
                // not re-arm a fresh confirmer (tokens persist in `Complete`).
                self.reciprocity_outcome = Some(confirmer.reciprocity());
                self.reciprocity_confirmer = None;
            }
        }

        // Sync engine step from session state
        match session.state() {
            vauchi_core::exchange::ExchangeState::Complete { contact } => {
                // Capture the rich success summary (what they shared + what
                // we shared) from the just-completed session before the
                // step flips — the engine owns no storage, so this reads the
                // session contact + the confirmed field-preview directly.
                if self.success_summary.is_none() {
                    self.success_summary = Some(build_legacy_success_summary(
                        contact,
                        self.field_preview.as_ref(),
                    ));
                }
                // Create reciprocity confirmer from session tokens.
                // Don't transition to Success until reciprocity is confirmed —
                // this prevents asymmetric exchanges where one side saves a
                // contact but the other never received the data.
                if self.reciprocity_confirmer.is_none()
                    && self.reciprocity_outcome.is_none()
                    && let (Some(our_token), Some(their_token)) = (
                        session.our_confirmation_token().copied(),
                        session.expected_their_token().copied(),
                    )
                    && let Some((gate, our_slot, their_slot)) = session.confirmation_escrow()
                {
                    let mut confirmer = ReciprocityConfirmer::new(
                        our_token,
                        their_token,
                        gate.to_string(),
                        our_slot.to_string(),
                        their_slot.to_string(),
                        self.clock.unix_seconds(),
                        true,
                    );
                    commands.extend(confirmer.start());
                    self.reciprocity_confirmer = Some(confirmer);
                    // Stay on Verifying while waiting for peer confirmation
                    self.step = ExchangeStep::Verifying;
                } else if self.reciprocity_outcome.is_some() {
                    // Reciprocity resolved. Both Confirmed and Pending keep the
                    // received contact: at Complete the peer's card + keys are
                    // already in hand; Pending only means the peer's save of
                    // ours is unconfirmed, which relay sync resolves later. Done
                    // persists via complete_exchange. Failed is reserved for a
                    // true protocol failure (the ExchangeState::Failed arm).
                    self.step = ExchangeStep::Success;
                } else if self.reciprocity_confirmer.is_some() {
                    // Confirmer still running — stay on Verifying
                    self.step = ExchangeStep::Verifying;
                } else {
                    // No confirmation tokens available (e.g., no relay configured).
                    // Fall through to Success for backward compat — but log a warning.
                    // This path should be eliminated once relay is always available.
                    self.step = ExchangeStep::Success;
                }
            }
            vauchi_core::exchange::ExchangeState::Failed { error } => {
                self.failure_detail = Some(error.user_message().to_string());
                self.step = ExchangeStep::Failed;
            }
            // Any state beyond DisplayingQr means verification is in progress
            vauchi_core::exchange::ExchangeState::PeerScanned { .. }
            | vauchi_core::exchange::ExchangeState::AwaitingKeyAgreement { .. }
            | vauchi_core::exchange::ExchangeState::AwaitingCardExchange { .. }
            | vauchi_core::exchange::ExchangeState::AwaitingNfcTap
            | vauchi_core::exchange::ExchangeState::AwaitingBleConnection
            | vauchi_core::exchange::ExchangeState::AwaitingBleVerification { .. } => {
                self.step = ExchangeStep::Verifying;
            }
            _ => {}
        }

        if commands.is_empty() {
            Some(ActionResult::UpdateScreen(self.build_screen()))
        } else {
            Some(ActionResult::Commands { commands })
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        // Mode selection — delegated to ModeSelectionEngine
        if self.step == ExchangeStep::ModeSelection {
            // BACK/Cancel on the mode-selection screen exits the flow.
            // `ModeSelection` is the flow root (back_nav.rs: deliberately
            // not back-safe), so a cancel must Complete — AppEngine's
            // `handle_completion` routes `AppScreen::Exchange` off to
            // Contacts/MyInfo. Without this the press fell through to
            // `ModeSelectionEngine` → `ModeSelectionResult::Screen` →
            // `UpdateScreen(same)`, a dead BACK (Fix C of
            // `2026-06-02-exchange-back-cancel-broken`). Mirrors the
            // `NfcRoleSelection` cancel arm below.
            if let UserAction::ActionPressed { action_id } = &action
                && action_id == "cancel"
            {
                return ActionResult::Complete;
            }
            if let Some(ref ms) = self.mode_selection {
                match ms.handle_action(&action) {
                    ModeSelectionResult::Selected(mode) => {
                        self.config.mode = Some(mode);
                        self.mode_selection = None;
                        // Record the selection step so a BACK press from
                        // the sub-flow rewinds here (see navigate_back_within).
                        self.step_history.push(ExchangeStep::ModeSelection);
                        // G4 (group-first): group selection already happened
                        // before mode. Show the field preview if the user chose
                        // "continue" at the group step (even with no groups
                        // toggled); "skip" goes straight to the sub-flow.
                        if self.show_preview_after_mode {
                            self.field_preview = Some(self.build_field_preview_config());
                            self.step = ExchangeStep::FieldPreview;
                            return ActionResult::NavigateTo(self.build_screen());
                        }
                        return self.enter_mode_sub_flow();
                    }
                    ModeSelectionResult::Screen(screen) => {
                        return ActionResult::UpdateScreen(*screen);
                    }
                }
            }
            return ActionResult::UpdateScreen(self.build_screen());
        }

        match (&self.step, action) {
            // Group selection: toggle group membership
            (
                ExchangeStep::GroupSelection,
                UserAction::ItemToggled {
                    component_id,
                    item_id,
                },
            ) if component_id == "group_picker" => {
                if let Some(pos) = self.selected_groups.iter().position(|g| g == &item_id) {
                    self.selected_groups.remove(pos);
                } else {
                    self.selected_groups.push(item_id);
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            // Group selection: continue or skip
            (ExchangeStep::GroupSelection, UserAction::ActionPressed { action_id })
                if action_id == "continue" || action_id == "skip" =>
            {
                // Record the group step so BACK from mode / preview / sub-flow
                // rewinds here (see navigate_back_within).
                self.step_history.push(ExchangeStep::GroupSelection);
                // "continue" shows the field preview after mode; "skip" clears
                // the chosen groups and skips the preview.
                self.show_preview_after_mode = action_id == "continue";
                if action_id == "skip" {
                    self.selected_groups.clear();
                }
                // G4 (group-first): mode selection comes after groups. When the
                // mode was pre-set (deep link / tests) there is no picker — go
                // straight to the field preview (continue) or the sub-flow
                // (skip); otherwise show the mode picker.
                if self.config.mode.is_some() {
                    if self.show_preview_after_mode {
                        self.field_preview = Some(self.build_field_preview_config());
                        self.step = ExchangeStep::FieldPreview;
                        return ActionResult::NavigateTo(self.build_screen());
                    }
                    return self.enter_mode_sub_flow();
                }
                self.mode_selection = Some(ModeSelectionEngine::new(
                    self.config.device_capabilities.clone(),
                    self.config.transport_readiness.clone(),
                ));
                self.step = ExchangeStep::ModeSelection;
                ActionResult::NavigateTo(self.build_screen())
            }
            // Field preview actions
            (ExchangeStep::FieldPreview, ref user_action) => {
                if let Some(outcome) = field_preview::handle_field_preview_action(user_action) {
                    match outcome {
                        FieldPreviewResult::StartExchange => {
                            // Route to the mode-specific sub-flow via the
                            // shared router (keeps grouped TapTap/Glance/Hover
                            // on their NFC/multi-stage screens).
                            return self.enter_mode_sub_flow();
                        }
                        FieldPreviewResult::ChangeGroups => {
                            self.field_preview = None;
                            self.step = ExchangeStep::GroupSelection;
                            return ActionResult::NavigateTo(self.build_screen());
                        }
                    }
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            (ExchangeStep::Success, UserAction::ActionPressed { action_id })
                if action_id == "done" =>
            {
                ActionResult::Complete
            }
            (ExchangeStep::Failed, UserAction::ActionPressed { action_id })
                if action_id == "retry" =>
            {
                self.scanned_data = None;
                self.failure_detail = None;
                self.ble_fallback_available = false;
                self.qr_fallback_available = false;
                // Restore the correct sub-flow for the selected mode via the
                // shared router: TapTap → NFC role chooser, Glance/Hover →
                // multi-stage, BLE → BLE, Link → link, else → QR. Previously
                // only Link/BLE were special-cased and TapTap/Glance/Hover fell
                // through to QR (the core!1041 divergence — missed 4th site).
                self.enter_mode_sub_flow()
            }
            (ExchangeStep::Failed, UserAction::ActionPressed { action_id })
                if action_id == "fallback_qr" =>
            {
                self.ble_fallback_available = false;
                self.qr_fallback_available = false;
                self.failure_detail = None;
                // "Fall back to QR" routes to the graduated Glance
                // (multi-stage QR), NOT the retired legacy QR sub-flow — the
                // same fix P2.D applied to the picker path (the legacy QR is
                // frozen on android). enter_mode_sub_flow routes Glance ->
                // ActionResult::StartMultiStageExchange.
                self.config.mode = Some(ExchangeMode::Glance);
                self.enter_mode_sub_flow()
            }
            (ExchangeStep::Failed, UserAction::ActionPressed { action_id })
                if action_id == "fallback_relay" =>
            {
                self.ble_fallback_available = false;
                self.qr_fallback_available = false;
                self.failure_detail = None;
                // INVARIANT: relay fallback bypasses the T2.3 enter_mode_sub_flow
                // guard, but that is safe — Link requires only Internet, which is
                // presence-only and has no OS permission concept
                // (`requirement_for_transport` maps no "internet" label, so it can
                // never be `PermissionRequired`). No denial can gate the relay.
                self.start_link_mode()
            }
            (ExchangeStep::Failed, UserAction::ActionPressed { action_id })
                if action_id == "cancel" =>
            {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// Build the shared exchange-success summary for the legacy engine from
/// the session's completed `contact` and the field-preview the user
/// confirmed. Free fn (not `&self`) so it composes with the live
/// `&mut self.session` borrow at the success-sync site. Link mode assigns
/// no group here, so `group_names` is empty (the legacy flow assigns
/// groups on Done, after this screen).
fn build_legacy_success_summary(
    contact: &vauchi_core::Contact,
    field_preview: Option<&FieldPreviewConfig>,
) -> crate::ui::exchange::success::ExchangeSuccessSummary {
    let card = contact.card();
    let received_fields = card
        .fields()
        .iter()
        .map(|f| {
            (
                format!("{:?}", f.field_type()),
                f.label().to_string(),
                f.value().to_string(),
            )
        })
        .collect();
    // What *we* shared, from the confirmed preview. `None` = no group
    // filter (share all); `Some(set)` = exactly that set. No preview (mode
    // skipped it) → unknown → empty.
    let my_visible_fields = field_preview
        .map(|fp| {
            fp.card
                .fields()
                .iter()
                .filter(|f| match &fp.visible_field_ids {
                    None => true,
                    Some(allow) => allow.contains(f.id()),
                })
                .map(|f| f.label().to_string())
                .collect()
        })
        .unwrap_or_default();
    crate::ui::exchange::success::ExchangeSuccessSummary {
        peer_name: card.display_name().to_string(),
        received_fields,
        my_visible_fields,
        group_names: Vec::new(),
    }
}

// INLINE_TEST_REQUIRED: Tests access private ExchangeStep enum and ExchangeEngine internals
#[cfg(test)]
mod tests {
    use super::*;

    fn config_no_groups() -> ExchangeConfig {
        ExchangeConfig {
            own_name: "Alice".into(),
            own_qr_data: "qr-data".into(),
            available_groups: vec![],
            device_capabilities: DeviceCapabilities::default(),
            // Pre-set mode to skip mode selection (tests focus on QR flow)
            mode: Some(ExchangeMode::Glance),
            card_snapshot: None,
            transport_readiness: Default::default(),
            available_group_data: Vec::new(),
        }
    }

    fn config_with_groups() -> ExchangeConfig {
        ExchangeConfig {
            own_name: "Alice".into(),
            own_qr_data: "qr-data".into(),
            available_groups: vec![
                ("g1".into(), "Family".into()),
                ("g2".into(), "Friends".into()),
            ],
            device_capabilities: DeviceCapabilities::default(),
            // TapHoverShake — a grouped multi-stage mode (graduated in P2.D of
            // the TapHoverShake plan; routes to multi-stage even with groups,
            // like Glance). Carries the group-selection machinery tests.
            mode: Some(ExchangeMode::TapHoverShake),
            card_snapshot: None,
            transport_readiness: Default::default(),
            available_group_data: Vec::new(),
        }
    }

    // @internal
    #[test]
    fn legacy_success_summary_extracts_peer_received_and_shared_fields() {
        use std::collections::HashSet;
        use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
        // Peer's card (what they shared).
        let mut peer = ContactCard::new("Bob");
        peer.add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "bob@example.com",
            0,
        ))
        .unwrap();
        let contact =
            vauchi_core::Contact::from_import(peer, vauchi_core::ImportSource::VcardFile, None, 0);

        // Our confirmed preview (what we shared) — only the phone field visible.
        let mut mine = ContactCard::new("Alice");
        mine.add_field(ContactField::new(
            FieldType::Phone,
            "Phone",
            "+1234567890",
            0,
        ))
        .unwrap();
        mine.add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "alice@example.com",
            0,
        ))
        .unwrap();
        let phone_id = mine.fields()[0].id().to_string();
        let preview = FieldPreviewConfig {
            card: mine,
            display_name: "Alice".into(),
            visible_field_ids: Some(HashSet::from([phone_id])),
        };

        let summary = build_legacy_success_summary(&contact, Some(&preview));
        assert_eq!(summary.peer_name, "Bob");
        assert_eq!(summary.received_fields.len(), 1, "one shared peer field");
        assert_eq!(summary.received_fields[0].1, "Email");
        assert_eq!(summary.received_fields[0].2, "bob@example.com");
        assert_eq!(
            summary.my_visible_fields,
            vec!["Phone".to_string()],
            "only the phone field was marked visible in the preview",
        );
        assert!(summary.group_names.is_empty());
    }

    // @internal
    #[test]
    fn success_screen_renders_rich_summary_when_attached() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.success_summary = Some(crate::ui::exchange::success::ExchangeSuccessSummary {
            peer_name: "Bob".into(),
            received_fields: vec![("Email".into(), "Email".into(), "bob@example.com".into())],
            my_visible_fields: vec!["Phone".into()],
            group_names: Vec::new(),
        });
        engine.step = ExchangeStep::Success;
        let screen = engine.build_screen();
        assert_eq!(screen.screen_id, "exchange_success");
        assert!(
            screen.components.iter().any(|c| matches!(
                c,
                Component::FieldList { id, .. } if id == "received_fields"
            )),
            "rich success screen renders the received card fields",
        );
        assert!(
            screen.components.iter().any(|c| matches!(
                c,
                Component::InfoPanel { id, .. } if id == "my_visibility"
            )),
            "rich success screen renders the visibility section",
        );
    }

    // @internal
    #[test]
    fn success_screen_without_summary_renders_minimal_chrome() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.step = ExchangeStep::Success;
        let screen = engine.build_screen();
        assert_eq!(screen.screen_id, "exchange_success");
        assert!(
            !screen.components.iter().any(|c| matches!(
                c,
                Component::FieldList { id, .. } if id == "received_fields"
            )),
            "minimal success screen has no received-fields section",
        );
        assert!(
            screen.components.iter().any(|c| matches!(
                c,
                Component::StatusIndicator { id, .. } if id == "success_status"
            )),
            "minimal success screen keeps its StatusIndicator",
        );
    }

    fn config_mode_selection() -> ExchangeConfig {
        ExchangeConfig {
            own_name: "Alice".into(),
            own_qr_data: "qr-data".into(),
            available_groups: vec![],
            device_capabilities: DeviceCapabilities {
                has_camera: true,
                has_internet: true,
                ..Default::default()
            },
            mode: None, // triggers mode selection
            card_snapshot: None,
            transport_readiness: Default::default(),
            available_group_data: Vec::new(),
        }
    }

    #[test]
    fn test_with_groups_starts_at_selection() {
        let engine = ExchangeEngine::new(
            config_with_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        // Should start at GroupSelection when groups exist
        assert_eq!(engine.step, ExchangeStep::GroupSelection);
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_group_selection");
    }

    #[test]
    fn test_group_selection_toggle_and_continue() {
        let mut engine = ExchangeEngine::new(
            config_with_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );

        // Toggle first group on
        let result = engine.handle_action(UserAction::ItemToggled {
            component_id: "group_picker".into(),
            item_id: "g1".into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));

        // Continue → field preview (not directly to QR)
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert!(matches!(result, ActionResult::NavigateTo(_)));
        assert_eq!(engine.step, ExchangeStep::FieldPreview);

        // Selected groups should be remembered
        assert_eq!(engine.selected_groups(), &["g1".to_string()]);
    }

    #[test]
    fn test_group_selection_skip() {
        let mut engine = ExchangeEngine::new(
            config_with_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );

        // Skip without selecting any groups
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "skip".into(),
        });
        // TapHoverShake graduated (P2.D) — skipping groups hands off to the
        // multi-stage engine instead of the retired legacy QR sub-flow.
        assert!(
            matches!(
                result,
                ActionResult::StartMultiStageExchange {
                    mode: ExchangeMode::TapHoverShake,
                },
            ),
            "skip must hand off to multi-stage; got {result:?}",
        );
        assert!(engine.selected_groups().is_empty());
    }

    // @internal
    #[test]
    fn test_group_selection_toggle_item_has_a11y() {
        // d4-a11y phase 2: the group-picker ToggleItem leaves carry a11y
        // (mirroring backup_recovery's toggle). The ToggleList container
        // and the continue/skip ScreenActions intentionally stay None —
        // a button's `label` is its accessible name.
        let engine = ExchangeEngine::new(
            config_with_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_group_selection");

        let items = screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::ToggleList {
                    id, items, a11y, ..
                } if id == "group_picker" => {
                    assert!(a11y.is_none(), "ToggleList container must not carry a11y");
                    Some(items)
                }
                _ => None,
            })
            .expect("group_picker ToggleList present");

        // First group is g1 "Family" (config_with_groups).
        let family = &items[0];
        assert_eq!(family.id, "g1");
        let a11y = family.a11y.as_ref().expect("ToggleItem must carry a11y");
        assert_eq!(a11y.label.as_deref(), Some("Family group toggle"));
        assert_eq!(
            a11y.hint.as_deref(),
            Some("Toggle to assign the new contact to this group.")
        );
        assert_eq!(a11y.role, Some(AccessibilityRole::Toggle));

        // The continue/skip actions intentionally have no a11y.
        for action in &screen.actions {
            assert!(
                action.a11y.is_none(),
                "ScreenAction {} must not carry a11y (label is the accessible name)",
                action.id
            );
        }
    }

    // ── ADR-031: ExchangeSession integration tests ──────────────────

    fn create_test_session() -> vauchi_core::exchange::ExchangeSession {
        let identity = vauchi_core::identity::Identity::create(
            "TestUser",
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        );
        let card = vauchi_core::contact_card::ContactCard::new("TestUser");
        let proximity = vauchi_core::exchange::ManualConfirmationVerifier::new();
        vauchi_core::exchange::ExchangeSession::new_qr(
            identity,
            card,
            proximity,
            vauchi_core::clock::SystemClock::shared(),
        )
    }

    #[test]
    fn test_with_session_group_continue_shows_field_preview() {
        let session = create_test_session();
        let mut engine = ExchangeEngine::with_session(
            config_with_groups(),
            session,
            vauchi_core::clock::SystemClock::shared(),
        );

        // Continue from group selection → FieldPreview (not QR directly)
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert!(
            matches!(result, ActionResult::NavigateTo(_)),
            "Expected NavigateTo for field preview"
        );
        assert_eq!(engine.step, ExchangeStep::FieldPreview);

        // Start exchange from field preview → multi-stage handoff.
        // TapHoverShake graduated in P2.D; the legacy QrDisplay path is gone
        // for this mode. The FieldPreview step itself is still reached above.
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "start_exchange".into(),
        });
        assert!(
            matches!(
                result,
                ActionResult::StartMultiStageExchange {
                    mode: ExchangeMode::TapHoverShake,
                },
            ),
            "start_exchange must hand off to multi-stage; got {result:?}",
        );
    }

    /// Drive a QR `ExchangeSession` to `Complete` (peer card received, keys
    /// agreed, escrow tokens derived). Returns the session and its escrow
    /// gate (hex) for building matching `RelayEscrow*` events.
    fn qr_session_at_complete() -> (vauchi_core::exchange::ExchangeSession, String) {
        use vauchi_core::contact_card::ContactCard;
        use vauchi_core::exchange::{
            ExchangeEvent, ExchangeSession, ManualConfirmationVerifier, ProximityConfidence,
        };
        use vauchi_core::identity::Identity;
        let clock = vauchi_core::clock::SystemClock::shared();
        let mut alice = ExchangeSession::new_qr(
            Identity::create("Alice", 0),
            ContactCard::new("Alice"),
            ManualConfirmationVerifier::new(),
            clock.clone(),
        );
        let mut bob = ExchangeSession::new_qr(
            Identity::create("Bob", 1),
            ContactCard::new("Bob"),
            ManualConfirmationVerifier::new(),
            clock.clone(),
        );
        alice.apply(ExchangeEvent::StartQR).unwrap();
        bob.apply(ExchangeEvent::StartQR).unwrap();
        let qr_bob = bob.qr().unwrap().clone();
        // Mirror the engine's own QR auto-advance sequence (handle_hardware_event).
        alice.apply(ExchangeEvent::ProcessQR(qr_bob)).unwrap();
        alice.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
        alice
            .apply(ExchangeEvent::ProximityCheckCompleted {
                confidence: ProximityConfidence::Medium,
            })
            .unwrap();
        alice.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
        alice
            .apply(ExchangeEvent::CompleteExchange(ContactCard::new("Alice")))
            .unwrap();
        assert!(
            matches!(
                alice.state(),
                vauchi_core::exchange::ExchangeState::Complete { .. }
            ),
            "session must reach Complete, got {:?}",
            alice.state()
        );
        let gate = alice
            .confirmation_escrow()
            .expect("escrow tokens available at Complete")
            .0
            .to_string();
        (alice, gate)
    }

    // Regression: a Pending reciprocity outcome (transient relay hiccup)
    // must NOT route to the Failed screen. At Complete the peer's card is
    // fully received; Pending only means the peer's save is unconfirmed.
    // Failed discarded the received contact (the received data + ratchet),
    // the data-loss bug from 2026-06-04-exchange-terminal-screens (New
    // finding) / 2026-07-03 GUI audit ranked #2. Map Pending to Success so
    // the Done handler persists via complete_exchange.
    // @internal
    #[test]
    fn pending_reciprocity_routes_to_success_not_failed() {
        let (session, gate) = qr_session_at_complete();
        let gate_hash = hex::decode(&gate).expect("escrow gate is hex");
        let mut engine = ExchangeEngine::with_session(
            config_no_groups(),
            session,
            vauchi_core::clock::SystemClock::shared(),
        );

        // First escrow event: the Complete session no-ops it; the engine
        // creates the reciprocity confirmer and moves to Verifying.
        let _ = engine.handle_hardware_event(vauchi_core::Event::RelayEscrowReady {
            gate_hash: gate_hash.clone(),
        });
        assert_eq!(
            engine.step,
            ExchangeStep::Verifying,
            "an active confirmer keeps the flow on Verifying"
        );

        // Peer deposited a non-matching blob → confirmer resolves Pending.
        let _ = engine.handle_hardware_event(vauchi_core::Event::RelayEscrowBlobReceived {
            gate_hash,
            blob: vec![0xFF; 32],
        });
        assert_eq!(
            engine.step,
            ExchangeStep::Success,
            "Pending reciprocity must land on Success (contact kept), not Failed/stuck-Verifying"
        );
    }

    // Happy path: a Confirmed reciprocity outcome must reach Success and stay
    // there — the confirmer must not be re-armed after it resolves (tokens
    // persist in Complete, so the pre-fix code looped back to Verifying).
    // @internal
    #[test]
    fn confirmed_reciprocity_reaches_and_stays_on_success() {
        let (session, gate) = qr_session_at_complete();
        let gate_hash = hex::decode(&gate).expect("escrow gate is hex");
        // The matching blob is our expected peer token — read it before the
        // session is moved into the engine.
        let expected_peer_token = *session
            .expected_their_token()
            .expect("expected-peer token available at Complete");
        let mut engine = ExchangeEngine::with_session(
            config_no_groups(),
            session,
            vauchi_core::clock::SystemClock::shared(),
        );

        let _ = engine.handle_hardware_event(vauchi_core::Event::RelayEscrowReady {
            gate_hash: gate_hash.clone(),
        });
        assert_eq!(engine.step, ExchangeStep::Verifying);

        let _ = engine.handle_hardware_event(vauchi_core::Event::RelayEscrowBlobReceived {
            gate_hash,
            blob: expected_peer_token.to_vec(),
        });
        assert_eq!(
            engine.step,
            ExchangeStep::Success,
            "Confirmed reciprocity must reach Success"
        );

        // A further escrow event must not re-arm the confirmer / bounce back
        // to Verifying — the outcome is latched.
        let _ = engine.handle_hardware_event(vauchi_core::Event::RelayEscrowReady {
            gate_hash: hex::decode(&gate).expect("hex"),
        });
        assert_eq!(
            engine.step,
            ExchangeStep::Success,
            "resolved exchange must stay on Success, not re-arm the confirmer"
        );
    }

    #[test]
    fn test_selected_groups_persists_through_exchange() {
        let mut engine = ExchangeEngine::new(
            config_with_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );

        // Select a group
        let _ = engine.handle_action(UserAction::ItemToggled {
            component_id: "group_picker".into(),
            item_id: "g2".into(),
        });
        // Continue → FieldPreview
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert_eq!(engine.step, ExchangeStep::FieldPreview);

        // Start exchange from FieldPreview → multi-stage handoff
        // (TapHoverShake graduated in P2.D). The selected groups must survive
        // the handoff; the legacy ShowQr → ScanQr → Verifying walk no longer
        // applies to this mode (router-driven modes all left the QR sub-flow).
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "start_exchange".into(),
        });
        assert!(
            matches!(
                result,
                ActionResult::StartMultiStageExchange {
                    mode: ExchangeMode::TapHoverShake,
                },
            ),
            "start_exchange must hand off to multi-stage; got {result:?}",
        );

        // Groups still selected at (and through) the handoff.
        assert_eq!(engine.selected_groups(), &["g2".to_string()]);
    }

    // ── T1-2: Exchange error detail tests ──────────────────────────

    #[test]
    fn failed_screen_shows_error_detail_after_mark_failed_with_error() {
        let mut engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.mark_failed_with_error(&vauchi_core::exchange::ExchangeError::SessionTimeout);
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_failed");
        let detail = screen.components.iter().find_map(|c| match c {
            Component::StatusIndicator { detail, .. } => detail.clone(),
            _ => None,
        });
        assert_eq!(
            detail.as_deref(),
            Some("The exchange timed out. Please try again."),
            "Failed screen should show user-friendly error detail"
        );
    }

    #[test]
    fn failed_screen_has_no_detail_after_plain_mark_failed() {
        let mut engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.mark_failed();
        let screen = engine.current_screen();
        let detail = screen.components.iter().find_map(|c| match c {
            Component::StatusIndicator { detail, .. } => detail.clone(),
            _ => None,
        });
        assert!(
            detail.is_none(),
            "Plain mark_failed should have no detail (backward-compatible)"
        );
    }

    #[test]
    fn retry_clears_failure_detail() {
        let mut engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        // TapHoverShake graduated to the multi-stage engine (P2.D), so retry
        // routes through the shared enter_mode_sub_flow router and hands off
        // to multi-stage. The point here is that Retry clears the failure
        // detail; all router-driven modes now leave the legacy QR sub-flow.
        engine.config.mode = Some(ExchangeMode::TapHoverShake);
        engine.mark_failed_with_error(&vauchi_core::exchange::ExchangeError::BleOutOfRange);
        assert!(engine.failure_detail.is_some());

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "retry".into(),
        });

        assert!(
            engine.failure_detail.is_none(),
            "Retry should clear the failure detail"
        );
        assert!(
            matches!(
                result,
                ActionResult::StartMultiStageExchange {
                    mode: ExchangeMode::TapHoverShake,
                },
            ),
            "TapHoverShake retry must hand off to multi-stage; got {result:?}",
        );
    }

    // Regression: `Cable` (USB direct transport) was dropped into the legacy
    // QR catch-all by the picker entry, rendering frozen on android
    // (`2026-06-03-android-animated-qr-stuck-frame-zero`). Must route Direct.
    // @internal
    #[test]
    fn cable_routes_to_direct_transport_not_qr() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.config.mode = Some(ExchangeMode::Cable);
        engine.mark_failed();

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "retry".into(),
        });

        // Cable graduated to the dedicated `DirectTransportEngine`; retry now
        // emits the navigation signal rather than entering a legacy USB step
        // (and so can never render the frozen-on-android legacy QR screen).
        assert!(
            matches!(result, ActionResult::StartDirectTransport),
            "Cable retry routes to the dedicated DirectTransportEngine, got {result:?}"
        );
    }

    #[test]
    fn user_message_covers_all_error_categories() {
        use vauchi_core::exchange::ExchangeError;
        // Verify a representative from each category returns non-empty
        let cases = vec![
            ExchangeError::QRExpired,
            ExchangeError::SessionTimeout,
            ExchangeError::ProximityFailed,
            ExchangeError::DuplicateContact,
            ExchangeError::ConsentDenied,
            ExchangeError::NetworkDisconnected,
            ExchangeError::LowBattery,
            ExchangeError::ClockDrift(300),
            ExchangeError::BleOutOfRange,
            ExchangeError::BleNotAvailable,
            ExchangeError::NfcNotSupported,
            ExchangeError::CryptoError,
            ExchangeError::HardwareFailure {
                transport: "BLE".into(),
                error: "test".into(),
            },
        ];
        for error in &cases {
            let msg = error.user_message();
            assert!(
                !msg.is_empty(),
                "user_message should be non-empty for {:?}",
                error
            );
            assert!(
                !msg.contains("Error("),
                "user_message should not expose internal types for {:?}",
                error
            );
        }
    }

    // ── Mode selection integration tests ───────────────────────────

    #[test]
    fn mode_none_starts_at_mode_selection() {
        let engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        assert_eq!(engine.step, ExchangeStep::ModeSelection);
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_mode_selection");
    }

    #[test]
    fn mode_selection_pick_advances_past_selection_via_ble_handoff() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        assert_eq!(engine.step, ExchangeStep::ModeSelection);

        // Pick Glance mode — G3 hands this off to the BLE engine.
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:quick".into(),
            item_id: "mode:glance".into(),
        });

        // Mode is recorded in config; the engine remains on ModeSelection
        // because the flow leaves Exchange entirely once AppEngine routes
        // the StartBleExchange result.
        assert_eq!(engine.config.mode, Some(ExchangeMode::Glance));
        assert_eq!(engine.step, ExchangeStep::ModeSelection);
        assert!(
            matches!(result, ActionResult::StartBleExchange { .. }),
            "Expected StartBleExchange handoff (G3), got {:?}",
            result
        );
    }

    // ── T2.3: fail fast before entering a wait a denied permission can't win ──

    /// All hardware present, so a denial yields PermissionRequired (present but
    /// denied) — the exact case the T2.3 guard targets (absent hardware would be
    /// Unavailable, with no grant path).
    fn all_present_caps() -> DeviceCapabilities {
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

    /// Mode-picker config (mode: None) with all hardware present and a
    /// caller-supplied permission ledger.
    fn failfast_config(ledger: TransportReadiness) -> ExchangeConfig {
        ExchangeConfig {
            own_name: "Alice".into(),
            own_qr_data: "qr".into(),
            available_groups: vec![],
            device_capabilities: all_present_caps(),
            transport_readiness: ledger,
            mode: None,
            card_snapshot: None,
            available_group_data: Vec::new(),
        }
    }

    /// Pick `item_id` on the picker; returns the engine + routing result.
    fn pick_mode(ledger: TransportReadiness, item_id: &str) -> (ExchangeEngine, ActionResult) {
        let mut engine = ExchangeEngine::new(
            failfast_config(ledger),
            vauchi_core::clock::SystemClock::shared(),
        );
        assert_eq!(engine.step, ExchangeStep::ModeSelection);
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "recommended".into(),
            item_id: item_id.into(),
        });
        (engine, result)
    }

    /// Assert a routing result is the fail-fast screen naming `needle`.
    fn assert_failed_naming(result: ActionResult, engine: &ExchangeEngine, needle: &str) {
        let screen = match result {
            ActionResult::NavigateTo(s) => s,
            other => panic!("expected NavigateTo(exchange_failed), got {other:?}"),
        };
        assert_eq!(screen.screen_id, "exchange_failed");
        assert_eq!(engine.step, ExchangeStep::Failed);
        let action_ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
        assert!(
            action_ids.contains(&"retry") && action_ids.contains(&"cancel"),
            "fail-fast screen must offer retry + cancel, got {action_ids:?}"
        );
        let detail = engine.failure_detail.as_deref().unwrap_or_default();
        assert!(
            detail.contains(needle),
            "failure detail should name {needle:?}, got {detail:?}"
        );
    }

    // @internal
    #[test]
    fn denied_ble_routes_magic_to_failed_not_ble_wait() {
        let mut led = TransportReadiness::new();
        led.note_denied(DeviceRequirement::Ble);
        let (engine, result) = pick_mode(led, "mode:magic");
        assert_eq!(engine.config.mode, Some(ExchangeMode::Magic));
        assert!(
            !matches!(result, ActionResult::StartBleExchange { .. }),
            "denied BLE must NOT enter the BLE wait"
        );
        assert_failed_naming(result, &engine, "Bluetooth");
    }

    // @internal
    #[test]
    fn denied_nfc_routes_taptap_to_failed_not_nfc_wait() {
        let mut led = TransportReadiness::new();
        led.note_denied(DeviceRequirement::Nfc);
        let (engine, result) = pick_mode(led, "mode:tap_tap");
        assert!(
            !matches!(result, ActionResult::StartNfcExchange),
            "denied NFC must NOT enter the NFC wait"
        );
        assert_failed_naming(result, &engine, "NFC");
    }

    // @internal
    #[test]
    fn denied_microphone_routes_hover_to_failed_not_multistage() {
        // Hover requires Camera + Microphone + Speaker; deny only Microphone
        // (Camera/Speaker present-Unknown). PermissionRequired reports the FIRST
        // denied requirement, so the detail names the microphone, not camera.
        let mut led = TransportReadiness::new();
        led.note_denied(DeviceRequirement::Microphone);
        let (engine, result) = pick_mode(led, "mode:hover");
        assert!(
            !matches!(result, ActionResult::StartMultiStageExchange { .. }),
            "denied microphone must NOT enter the multi-stage wait"
        );
        assert_failed_naming(result, &engine, "Microphone");
    }

    // @internal
    #[test]
    fn unknown_ledger_lets_magic_enter_ble_wait() {
        // Optimistic-Unknown contract: an empty ledger never blocks entry.
        let (engine, result) = pick_mode(TransportReadiness::new(), "mode:magic");
        assert!(
            matches!(result, ActionResult::StartBleExchange { .. }),
            "unknown permission must still enter the wait, got {result:?}"
        );
        assert_ne!(engine.step, ExchangeStep::Failed);
    }

    // @internal
    #[test]
    fn granted_nfc_lets_taptap_enter_nfc_wait() {
        let mut led = TransportReadiness::new();
        led.note_granted(DeviceRequirement::Nfc);
        let (_engine, result) = pick_mode(led, "mode:tap_tap");
        assert!(
            matches!(result, ActionResult::StartNfcExchange),
            "granted NFC must enter the wait, got {result:?}"
        );
    }

    // @internal
    #[test]
    fn cable_never_blocks_even_with_a_poisoned_ledger() {
        // CC-14: Cable needs only UsbPort (presence-only, no permission). Even a
        // ledger denying every permission-bearing transport must not block it.
        let mut led = TransportReadiness::new();
        led.note_denied(DeviceRequirement::Ble);
        led.note_denied(DeviceRequirement::Camera);
        led.note_denied(DeviceRequirement::Nfc);
        led.note_denied(DeviceRequirement::Microphone);
        let (engine, result) = pick_mode(led, "mode:cable");
        assert!(
            matches!(result, ActionResult::StartDirectTransport),
            "Cable (UsbPort presence-only) must never fail-fast, got {result:?}"
        );
        assert_ne!(engine.step, ExchangeStep::Failed);
    }

    // @internal
    #[test]
    fn unrecognized_location_label_does_not_deny_magic() {
        // CC-14: "location" is the capture-geolocation permission, not a
        // transport; note_permission_denied ignores it, so Magic still enters
        // the BLE wait.
        let mut led = TransportReadiness::new();
        led.note_permission_denied("location");
        let (engine, result) = pick_mode(led, "mode:magic");
        assert!(
            matches!(result, ActionResult::StartBleExchange { .. }),
            "a location denial must not gate a BLE mode, got {result:?}"
        );
        assert_ne!(engine.step, ExchangeStep::Failed);
    }

    // @internal
    #[test]
    fn preset_mode_deep_link_path_fails_fast_on_denied_ble() {
        // The deep-link / preset-mode path reaches enter_mode_sub_flow via
        // group-skip, not the picker. The guard must still fire there.
        let mut led = TransportReadiness::new();
        led.note_denied(DeviceRequirement::Ble);
        let mut config = failfast_config(led);
        config.mode = Some(ExchangeMode::Magic);
        config.available_groups = vec![("g1".into(), "Work".into())];
        let mut engine = ExchangeEngine::new(config, vauchi_core::clock::SystemClock::shared());
        assert_eq!(engine.step, ExchangeStep::GroupSelection);
        // Skip groups → routes straight to the preset mode's sub-flow.
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "skip".into(),
        });
        assert!(
            !matches!(result, ActionResult::StartBleExchange { .. }),
            "preset-mode deep-link path must also fail fast on denied BLE"
        );
        assert_failed_naming(result, &engine, "Bluetooth");
    }

    // @internal
    #[test]
    fn retry_with_still_denied_permission_refails_not_enter_wait() {
        // Retry clears failure_detail then re-enters; with the ledger still
        // denied the guard must re-fail, not fall through to the BLE wait.
        let mut led = TransportReadiness::new();
        led.note_denied(DeviceRequirement::Ble);
        let (mut engine, _first) = pick_mode(led, "mode:magic");
        assert_eq!(engine.step, ExchangeStep::Failed);
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "retry".into(),
        });
        assert!(
            !matches!(result, ActionResult::StartBleExchange { .. }),
            "retry with a still-denied permission must re-fail, not enter the wait"
        );
        assert_failed_naming(result, &engine, "Bluetooth");
    }

    #[test]
    fn group_selection_precedes_mode_when_groups_exist() {
        // G4 (group-first): with groups present, the exchange begins at
        // GroupSelection; continue/skip then advances to mode selection.
        let mut config = config_mode_selection();
        config.available_groups = vec![("g1".into(), "Work".into())];
        let mut engine = ExchangeEngine::new(config, vauchi_core::clock::SystemClock::shared());
        assert_eq!(engine.step, ExchangeStep::GroupSelection);

        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "skip".into(),
        });
        assert_eq!(engine.step, ExchangeStep::ModeSelection);
    }

    // Fix C of `2026-06-02-exchange-back-cancel-broken`: BACK/Cancel on
    // the mode-selection screen must EXIT the exchange flow, not no-op.
    // `ModeSelection` is the flow root (back_nav.rs: deliberately not
    // back-safe), so exiting is `ActionResult::Complete` — which
    // `AppEngine::handle_completion` routes off `AppScreen::Exchange`.
    // Before the fix, `cancel` fell through `ModeSelectionEngine` to
    // `ModeSelectionResult::Screen` → `UpdateScreen(same)` → dead BACK.
    // Mirrors the existing `NfcRoleSelection` cancel arm.
    // @internal
    #[test]
    fn mode_selection_cancel_completes_to_exit() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        assert_eq!(engine.step, ExchangeStep::ModeSelection);

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "cancel".into(),
        });

        assert!(
            matches!(result, ActionResult::Complete),
            "Cancel on mode selection must Complete (exit the flow), got {:?}",
            result
        );
        // Negative (CC-11): it must NOT be the old no-op re-render.
        assert!(
            !matches!(result, ActionResult::UpdateScreen(_)),
            "Cancel must not re-render the same screen (dead-BACK regression)"
        );
    }

    // ── Hover / Glance mode tests ──────────────────────────────────

    // RED for Phase 1.E.2 of `2026-05-11-hover-graduation-plan.md`.
    //
    // After 1.E.3 GREEN, Hover stops routing through the legacy
    // `ExchangeStep::Qr` sub-flow — it joins Glance on the
    // `ActionResult::StartMultiStageExchange` handoff path, with
    // the engine staying on `ModeSelection` while AppEngine
    // navigates to `AppScreen::MultiStageExchange`. The legacy
    // `exchange_show_qr` screen is reached only by modes that
    // have *not* graduated to the new engine (TapHoverShake —
    // Phase 2/3 of the umbrella retirement).
    //
    // This test pins the unreachability of the legacy path for
    // Hover — a regression gate so a future refactor can't
    // silently re-route Hover back through `ExchangeStep::Qr`.
    // @internal
    #[test]
    fn hover_mode_does_not_advance_to_legacy_qr_step() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );

        // Pick Hover
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:standard".into(),
            item_id: "mode:hover".into(),
        });
        assert_eq!(engine.config.mode, Some(ExchangeMode::Hover));
        // Engine stays on ModeSelection — the flow leaves
        // Exchange entirely once AppEngine routes the
        // StartMultiStageExchange result.
        assert_eq!(engine.step, ExchangeStep::ModeSelection);
    }

    #[test]
    fn glance_mode_routes_to_ble_handoff() {
        // Pair 4 — Glance is the canonical bilateral simultaneous QR
        // mode and now hands off to the new core-driven
        // `MultiStageExchange` screen instead of the legacy
        // ExchangeStep::Qr sub-flow. The engine emits
        // `StartMultiStageExchange`; AppEngine routing translates it
        // into navigation, and PlatformAppEngine auto-creates the
        // session.
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );

        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:quick".into(),
            item_id: "mode:glance".into(),
        });
        assert_eq!(engine.config.mode, Some(ExchangeMode::Glance));
        // Engine stays on ModeSelection — the flow leaves Exchange
        // entirely once AppEngine routes the StartMultiStageExchange
        // result. Step is never advanced into Qr/Ble/Link sub-flows
        // for Glance.
        assert_eq!(engine.step, ExchangeStep::ModeSelection);
        // RED for Phase 1.E.2 — the unit variant becomes tagged
        // with the mode payload so AppEngine can pick the right
        // engine constructor (`new_glance` vs `new_hover`).
        assert!(
            matches!(
                result,
                ActionResult::StartBleExchange {
                    mode: ExchangeMode::Glance,
                },
            ),
            "Glance must hand off to BLE (G3) with mode payload; got {result:?}",
        );
    }

    // RED for Phase 1.E.2 of `2026-05-11-hover-graduation-plan.md`.
    //
    // Mirror of `glance_mode_routes_through_multi_stage_handoff`.
    // Hover graduates from the legacy `ExchangeStep::Qr` sub-flow
    // (pinned-unreachable in `hover_mode_does_not_advance_to_legacy_qr_step`)
    // onto the same `StartMultiStageExchange` path Pair 4
    // introduced for Glance. The mode payload carries
    // `ExchangeMode::Hover` so AppEngine constructs the engine via
    // `MultiStageExchangeEngine::new_hover()` — front-camera
    // default + autonomous audio-handshake trigger wired (the
    // 1.C polish commit gates the trigger on Hover-only via
    // `is_active_engine_multi_stage_hover()`).
    // @internal
    #[test]
    fn hover_mode_routes_through_multi_stage_handoff() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );

        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:standard".into(),
            item_id: "mode:hover".into(),
        });
        assert_eq!(engine.config.mode, Some(ExchangeMode::Hover));
        assert_eq!(engine.step, ExchangeStep::ModeSelection);
        assert!(
            matches!(
                result,
                ActionResult::StartMultiStageExchange {
                    mode: ExchangeMode::Hover,
                },
            ),
            "Hover must hand off to multi-stage with mode=Hover; got {result:?}",
        );
    }

    // P2.D of `2026-06-03-taphovershake-graduation-plan.md`. Mirror of
    // the Hover handoff: selecting TapHoverShake routes to the new
    // `MultiStageExchange` engine (mode payload `TapHoverShake`) instead
    // of the legacy `ExchangeStep::Qr` sub-flow — closing the android
    // frozen-QR bug, whose permanent fix is this graduation.
    // @internal
    #[test]
    fn tap_hover_shake_mode_routes_through_multi_stage_handoff() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );

        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:fun".into(),
            item_id: "mode:tap_hover_shake".into(),
        });
        assert_eq!(engine.config.mode, Some(ExchangeMode::TapHoverShake));
        assert_eq!(engine.step, ExchangeStep::ModeSelection);
        assert!(
            matches!(
                result,
                ActionResult::StartMultiStageExchange {
                    mode: ExchangeMode::TapHoverShake,
                },
            ),
            "TapHoverShake must hand off to multi-stage with mode=TapHoverShake; got {result:?}",
        );
    }

    #[test]
    fn field_preview_change_groups_returns_to_group_selection() {
        let mut config = config_mode_selection();
        config.available_groups = vec![("g1".into(), "Work".into())];
        let mut engine = ExchangeEngine::new(config, vauchi_core::clock::SystemClock::shared());

        // Group-first: the exchange begins at GroupSelection.
        assert_eq!(engine.step, ExchangeStep::GroupSelection);
        // Choose a group, then continue → ModeSelection.
        let _ = engine.handle_action(UserAction::ItemToggled {
            component_id: "group_picker".into(),
            item_id: "g1".into(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert_eq!(engine.step, ExchangeStep::ModeSelection);

        // Pick a mode → FieldPreview (groups were chosen).
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:quick".into(),
            item_id: "mode:glance".into(),
        });
        assert_eq!(engine.step, ExchangeStep::FieldPreview);

        // Change groups → back to GroupSelection
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "change_groups".into(),
        });
        assert_eq!(engine.step, ExchangeStep::GroupSelection);
    }

    // ── Link mode routing ─────────────────────────────────────────
    //
    // The link-mode initiator flow (share-url / waiting / retrieving /
    // terminal screens, escrow polling, card decrypt) graduated to the
    // pure `LinkExchangeEngine` + engine-owned `LinkInitiatorSession`
    // (slice 32l Phase 2/3). This engine no longer enters an
    // `ExchangeStep::Link` sub-flow — every Link entry point hands off
    // via `ActionResult::StartLinkExchange`, which AppEngine routes to
    // construct the new engine. The per-screen rendering + state
    // machine coverage lives in `tests/reachability/link_exchange.rs`
    // and `vauchi-core`'s `link_initiator` tests; here we pin only the
    // handoff.

    // @internal
    #[test]
    fn link_mode_pick_routes_to_link_exchange_handoff() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );

        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:remote".into(),
            item_id: "mode:link".into(),
        });
        assert_eq!(engine.config.mode, Some(ExchangeMode::Link));
        // Engine never advances into a Link sub-flow — the flow leaves
        // Exchange once AppEngine routes the handoff.
        assert_eq!(engine.step, ExchangeStep::ModeSelection);
        assert_eq!(
            result,
            ActionResult::StartLinkExchange,
            "Link mode-pick must hand off to LinkExchangeEngine; got {result:?}",
        );
    }

    // @internal
    #[test]
    fn link_mode_field_preview_start_routes_to_link_exchange() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_with_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        assert_eq!(engine.step, ExchangeStep::GroupSelection);

        // Choose a group so the continue path shows the field preview (mode is
        // pre-set, so there is no mode-selection step).
        let _ = engine.handle_action(UserAction::ItemToggled {
            component_id: "group_picker".into(),
            item_id: "g1".into(),
        });
        // Continue → FieldPreview
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert_eq!(engine.step, ExchangeStep::FieldPreview);

        // Start exchange → hand off to LinkExchangeEngine (not a Link sub-flow)
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "start_exchange".into(),
        });
        assert_eq!(
            result,
            ActionResult::StartLinkExchange,
            "FieldPreview start in Link mode must hand off; got {result:?}",
        );
        assert_eq!(engine.step, ExchangeStep::FieldPreview);
    }

    // @internal
    #[test]
    fn link_mode_retry_routes_to_link_exchange() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.mark_failed();
        assert_eq!(engine.step, ExchangeStep::Failed);

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "retry".into(),
        });
        assert_eq!(
            result,
            ActionResult::StartLinkExchange,
            "Retry in Link mode must hand off to LinkExchangeEngine; got {result:?}",
        );
    }

    // ── Scan quality tracking ──────────────────────────────────────

    // ── Grouped-card mode routing regression ───────────────────────
    //
    // `2026-06-02-grouped-mode-routing-nfc`: when the card has groups,
    // mode selection detours through GroupSelection first. The
    // resume-after-groups paths (group Skip; group Continue →
    // FieldPreview → start_exchange) must replicate the SAME
    // mode→sub-flow routing as the no-groups branch. Before the fix
    // they special-cased only the BLE modes and collapsed everything
    // else to the legacy QR step — so grouped TapTap silently lost its
    // NFC role-selection screen (and Glance/Hover lost the multi-stage
    // handoff). Surfaced on-device: iOS "Tap tap" → Assign-to-Groups →
    // Skip → QR instead of the Send/Receive role chooser.

    // @internal
    #[test]
    fn glance_with_groups_skip_routes_to_ble() {
        // Same bug class for the multi-stage modes: grouped Glance + Skip
        // must hand off to MultiStageExchange, not collapse to QR.
        let mut config = config_mode_selection();
        config.available_groups = vec![("g1".into(), "Work".into())];
        let mut engine = ExchangeEngine::new(config, vauchi_core::clock::SystemClock::shared());

        // Group-first: begins at GroupSelection. Skip groups → ModeSelection.
        assert_eq!(engine.step, ExchangeStep::GroupSelection);
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "skip".into(),
        });
        assert_eq!(engine.step, ExchangeStep::ModeSelection);

        // Pick Glance → hand off to multi-stage (no groups selected).
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:quick".into(),
            item_id: "mode:glance".into(),
        });
        assert!(
            matches!(
                result,
                ActionResult::StartBleExchange {
                    mode: ExchangeMode::Glance
                }
            ),
            "grouped Glance + skip must hand off to BLE (G3), got {result:?}"
        );
    }
}
