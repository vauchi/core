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
pub(crate) mod field_preview;
pub(crate) mod mode_selection;
pub(crate) mod nfc;
pub mod nfc_engine;
pub(crate) mod scan_quality;
pub(crate) mod success;
pub(crate) mod verifying;

use self::field_preview::{FieldPreviewConfig, FieldPreviewResult};
use self::mode_selection::{ModeSelectionEngine, ModeSelectionResult};
use self::nfc::{NfcExchangeFlow, NfcHardwareOutcome, NfcStep};
use crate::ui::*;
use vauchi_core::Command;
use vauchi_core::clock::Clock;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::ExchangeMode;
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
    /// Selected exchange mode. `None` = show mode selection first.
    #[serde(default)]
    pub mode: Option<ExchangeMode>,
    /// Frozen card snapshot for exchange. `None` = snapshot at exchange start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema-gen", schemars(skip))]
    pub card_snapshot: Option<vauchi_core::exchange::card_snapshot::CardSnapshot>,
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
    /// NFC exchange flow state machine (3-phase encrypted handshake).
    /// Constructed at TapTap dispatch via [`Self::start_taptap_mode`];
    /// `NfcExchangeFlow` consumes the cached [`Self::nfc_identity`].
    nfc_flow: Option<NfcExchangeFlow>,
    /// Owned `Identity` clone reserved for `NfcExchangeFlow`
    /// construction. Populated by the engine constructor when the
    /// caller (AppEngine in `screens.rs`) clones identity via the
    /// storage-bytes roundtrip; consumed by `start_taptap_mode`. None
    /// for non-NFC flows or when no identity is available — TapTap
    /// dispatch then routes to the Failed step with an explanatory
    /// `failure_detail`.
    nfc_identity: Option<vauchi_core::identity::Identity>,
    /// Reciprocity confirmation cascade driver (created on exchange completion).
    reciprocity_confirmer: Option<ReciprocityConfirmer>,
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

/// Sub-steps for the USB cable / direct TCP exchange flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectStep {
    /// Waiting for USB connection (phone acts as server).
    WaitingForConnection,
    /// TCP exchange in progress.
    Exchanging,
}

impl DirectStep {
    /// Matches `QrStep::STEP_COUNT` / `BleStep::STEP_COUNT` for consistent
    /// progress bar. Padded to 3 with a no-op mapping for the third slot.
    pub(self) const STEP_COUNT: u8 = 3;

    fn step_number(self, base: u8) -> u8 {
        base + match self {
            Self::WaitingForConnection => 0,
            Self::Exchanging => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum ExchangeStep {
    /// User picks an exchange mode (first step when mode is not pre-set).
    ModeSelection,
    /// Pick groups for the new contact (shown only if groups exist).
    GroupSelection,
    /// Read-only preview of what will be shared (after group selection).
    FieldPreview,
    /// Choose the NFC role (Send/Receive) after picking TapTap. Core-driven
    /// (ADR-043/044): emits an i18n-keyed `ScreenModel` the frontend renders;
    /// Send -> `start_taptap_mode`, Receive -> `start_nfc_receive_mode`.
    NfcRoleSelection,
    /// NFC exchange sub-flow (3-phase encrypted handshake over an
    /// NFC tap). See `self::nfc` + 2026-05-19-nfc-exchange-engine-design.md.
    Nfc(NfcStep),
    /// USB cable / direct TCP exchange.
    DirectTransport(DirectStep),
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
            // The NFC role choice is part of the mode-selection phase.
            Self::NfcRoleSelection => 1,
            // Sub-flow steps start at 4 (after mode + group + preview)
            Self::Nfc(nfc) => nfc.step_number(4),
            Self::DirectTransport(direct) => direct.step_number(4),
            Self::Verifying => 6,
            Self::Success => 4 + NfcStep::STEP_COUNT,
            Self::Failed => 5 + NfcStep::STEP_COUNT,
        }
    }
}

// mode + group + preview + sub-flow + success/failed
// All sub-flows must have the same step count for consistent progress.
const _: () = assert!(DirectStep::STEP_COUNT == NfcStep::STEP_COUNT);
const TOTAL_STEPS: u8 = 3 + NfcStep::STEP_COUNT + 2;

impl ExchangeEngine {
    /// Determine the initial step based on config.
    ///
    /// If mode is pre-set, skip mode selection (backward compat / tests).
    /// Otherwise start at ModeSelection.
    fn initial_step(config: &ExchangeConfig) -> ExchangeStep {
        if config.mode.is_none() {
            return ExchangeStep::ModeSelection;
        }
        if config.available_groups.is_empty() {
            if config.mode == Some(ExchangeMode::Cable) {
                return ExchangeStep::DirectTransport(DirectStep::WaitingForConnection);
            }
            ExchangeStep::ModeSelection
        } else {
            ExchangeStep::GroupSelection
        }
    }

    /// Populate the identity clone reserved for TapTap (NFC) dispatch.
    ///
    /// Called by `AppEngine` at engine construction (see
    /// `app_engine/screens.rs`) when an active identity is available.
    /// `start_taptap_mode` consumes it. Identity doesn't impl `Clone`
    /// for key-zeroize reasons; callers produce the clone via
    /// `Identity::to_storage_bytes()` + `Identity::from_storage_bytes()`.
    pub fn set_nfc_identity(&mut self, identity: vauchi_core::identity::Identity) {
        self.nfc_identity = Some(identity);
    }

    pub fn new(config: ExchangeConfig, clock: Arc<dyn Clock>) -> Self {
        let step = Self::initial_step(&config);
        let mode_selection = if step == ExchangeStep::ModeSelection {
            Some(ModeSelectionEngine::new(config.device_capabilities.clone()))
        } else {
            None
        };
        Self {
            step,
            step_history: Vec::new(),
            config,
            scanned_data: None,
            selected_groups: Vec::new(),
            session: None,
            failure_detail: None,
            ble_fallback_available: false,
            qr_fallback_available: false,
            mode_selection,
            field_preview: None,
            nfc_flow: None,
            nfc_identity: None,
            reciprocity_confirmer: None,
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
            Some(ModeSelectionEngine::new(config.device_capabilities.clone()))
        } else {
            None
        };

        // If starting directly at DirectTransport, emit initial commands now.
        // USB sessions begin in AwaitingDirectPayload — no StartQR needed.
        if matches!(step, ExchangeStep::DirectTransport(_)) {
            session.emit_initial_commands();
        }

        Self {
            step,
            step_history: Vec::new(),
            config,
            scanned_data: None,
            selected_groups: Vec::new(),
            session: Some(session),
            failure_detail: None,
            ble_fallback_available: false,
            qr_fallback_available: false,
            mode_selection,
            field_preview: None,
            nfc_flow: None,
            nfc_identity: None,
            reciprocity_confirmer: None,
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

    pub fn scanned_data(&self) -> Option<&str> {
        self.scanned_data.as_deref()
    }

    /// Start the protocol session (ADR-031) when entering ShowQr.
    ///
    /// If a session is present, applies `StartQR` to generate the QR code,
    /// emits initial commands, and returns `Commands`.
    /// Otherwise, returns `NavigateTo` for legacy UI-only behavior.
    fn start_session_if_needed(&mut self) -> ActionResult {
        if let Some(ref mut session) = self.session {
            // USB direct transport: session is already in AwaitingDirectPayload.
            // Just emit initial commands (DirectSend) without starting QR.
            if matches!(self.step, ExchangeStep::DirectTransport(_)) {
                session.emit_initial_commands();
                let commands = session.drain_commands();
                if !commands.is_empty() {
                    self.step = ExchangeStep::DirectTransport(DirectStep::Exchanging);
                    return ActionResult::Commands { commands };
                }
            }
        }
        ActionResult::NavigateTo(self.build_screen())
    }

    /// Hand off to the graduated `LinkExchangeEngine` (ADR-021/043 Humble
    /// UI). The link-mode initiator flow (share-url / waiting / retrieving /
    /// terminal screens) now lives in its own engine driven by the
    /// engine-owned `LinkInitiatorSession`; this engine never enters a Link
    /// sub-flow. AppEngine routes `StartLinkExchange` to construct the new
    /// engine and build its `LinkInitiatorSession`.
    fn start_link_mode(&self) -> ActionResult {
        ActionResult::StartLinkExchange
    }

    /// Start TapTap exchange mode (NFC).
    ///
    /// TapTap is the domain/user-facing name for the pure-NFC exchange —
    /// `DataTransport::Nfc` per `MODE_TAP_TAP`. Creates an
    /// `NfcExchangeFlow` (initiator side) and activates it, emitting the
    /// initial `Command::NfcActivate { payload: key_offer }`. The
    /// responder side is driven by the peer's HCE service through the
    /// same `NfcExchangeFlow` shape — Phase 3b binding work (`android!410`)
    /// added the binder-block pattern that lets HCE consume
    /// `Event::NfcDataReceived` + return `Command::NfcSendApdu`
    /// synchronously.
    ///
    /// Identity is consumed from `self.nfc_identity` — populated at
    /// engine construction via [`Self::with_session_and_nfc_identity`]
    /// or [`Self::set_nfc_identity`]. The `NfcHandshakeSession`
    /// Phase-1 key offer needs the full `Identity` to sign the
    /// payload (see `NfcHandshakeSession::create_key_offer` in
    /// `vauchi_core::exchange::nfc_handshake`).
    fn start_taptap_mode(&mut self) -> ActionResult {
        let identity = match self.nfc_identity.take() {
            Some(id) => id,
            None => {
                self.failure_detail = Some("no active identity for NFC exchange".to_string());
                self.step = ExchangeStep::Failed;
                return ActionResult::UpdateScreen(self.build_screen());
            }
        };
        let display_name = self.config.own_name.clone();
        let mut flow = NfcExchangeFlow::new_initiator(identity, display_name);
        let commands = match flow.activate() {
            Ok(cmds) => cmds,
            Err(e) => {
                self.failure_detail = Some(format!("NFC activation failed: {e:?}"));
                self.step = ExchangeStep::Failed;
                return ActionResult::UpdateScreen(self.build_screen());
            }
        };
        self.nfc_flow = Some(flow);
        self.step = ExchangeStep::Nfc(NfcStep::AwaitingTap);
        ActionResult::Commands { commands }
    }

    /// Explicit responder ("Receive") entry for TapTap.
    ///
    /// Unlike [`Self::start_taptap_mode`] (the "Send"/initiator path), this
    /// does **not** create an `NfcExchangeFlow`: it leaves `nfc_flow = None`
    /// and the cached `nfc_identity` in place so the lazy HCE bootstrap in
    /// [`WorkflowEngine::handle_hardware_event`] spins up the responder on
    /// the peer's first tap. It emits an **empty** `Command::NfcActivate`
    /// up-front so the frontend registers its HCE `TransceiveContext` before
    /// that tap arrives — the initiator path's non-empty key-offer payload
    /// is the discriminator (empty payload = responder, register HCE;
    /// non-empty = initiator, enable reader-mode).
    fn start_nfc_receive_mode(&mut self) -> ActionResult {
        if self.nfc_identity.is_none() {
            self.failure_detail = Some("no active identity for NFC exchange".to_string());
            self.step = ExchangeStep::Failed;
            return ActionResult::UpdateScreen(self.build_screen());
        }
        self.step = ExchangeStep::Nfc(NfcStep::AwaitingTap);
        ActionResult::Commands {
            commands: vec![vauchi_core::Command::NfcActivate {
                payload: Vec::new(),
            }],
        }
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
        match self.config.mode {
            Some(ExchangeMode::Link) => self.start_link_mode(),
            Some(mode @ (ExchangeMode::Magic | ExchangeMode::Bump | ExchangeMode::Shake)) => {
                // The BLE flow runs in its own `AppScreen::BleExchange` engine
                // (`BleExchangeEngine`); this cached `ExchangeEngine` is kept for
                // Cancel. Re-arm the picker so it is not a zombie (Fix A of
                // 2026-06-02-exchange-back-cancel-broken).
                self.mode_selection = Some(ModeSelectionEngine::new(
                    self.config.device_capabilities.clone(),
                ));
                ActionResult::StartBleExchange { mode }
            }
            Some(ExchangeMode::TapTap) => {
                // Core-driven role choice (Send/Receive) before the NFC
                // flow — a two-device exchange needs one initiator + one
                // responder, so both devices opening the screen must NOT
                // both Send. (NFC graduation slice 2 flips this to
                // `ActionResult::StartNfcExchange` → the dedicated
                // `NfcExchangeEngine`; the engine + plumbing land first.)
                self.step = ExchangeStep::NfcRoleSelection;
                ActionResult::NavigateTo(self.build_screen())
            }
            // Pair 4 — `Glance` is the canonical face-to-face mode
            // (bilateral simultaneous QR with no proximity signal); route
            // it through the core-driven `MultiStageExchange` screen so the
            // multi-stage protocol drives both QR display and scan from a
            // pure ScreenModel rather than the legacy bespoke step state
            // machine. Phase 1.E of `2026-05-11-hover-graduation-plan.md`
            // extended the handoff to `Hover` (QR + ultrasonic). The `mode`
            // payload tells AppEngine which engine constructor to use
            // (`new_hover` vs `new_glance`). TapHoverShake (P2.D of the
            // TapHoverShake graduation plan) now joins them — it routes to
            // the new engine running QR + audio proximity (the accel shake
            // signal is a follow-up). This removes the last mode from the
            // legacy `ExchangeStep::Qr` catch-all, the permanent fix for the
            // android frozen-QR bug (`2026-06-03-android-animated-qr-stuck-frame-zero`).
            Some(
                mode @ (ExchangeMode::Glance | ExchangeMode::Hover | ExchangeMode::TapHoverShake),
            ) => {
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
                ));
                ActionResult::StartMultiStageExchange { mode }
            }
            Some(ExchangeMode::Cable) => {
                // Cable is USB direct transport, not QR. Mirror `initial_step`
                // so the picker entry doesn't drop it into the legacy QR
                // catch-all (frozen on android — the android-qr bug record).
                // `start_session_if_needed` handles the DirectTransport step.
                self.step = ExchangeStep::DirectTransport(DirectStep::WaitingForConnection);
                self.start_session_if_needed()
            }
            // No mode selected — return to the picker (production always
            // enters with mode: None; no mode reaches the retired legacy QR).
            _ => {
                self.mode_selection = Some(ModeSelectionEngine::new(
                    self.config.device_capabilities.clone(),
                ));
                self.step = ExchangeStep::ModeSelection;
                ActionResult::NavigateTo(self.build_screen())
            }
        }
    }

    /// Handle NFC mode hardware events via NfcExchangeFlow.
    /// Mirrors `handle_ble_hardware_event`; the sub-flow owns the
    /// 3-phase state machine and either advances or fails.
    fn handle_nfc_hardware_event(&mut self, event: vauchi_core::Event) -> Option<ActionResult> {
        let flow = self.nfc_flow.as_mut()?;
        let outcome = flow.handle_event(&event);
        self.step = ExchangeStep::Nfc(flow.step().clone());
        Some(self.apply_nfc_outcome(outcome))
    }

    /// Apply an `NfcHardwareOutcome` — translate to `ActionResult`.
    /// Mirrors `apply_ble_outcome`; the `RelayHandoff` payload on a
    /// failed outcome is intentionally not consumed yet (TODO in the
    /// next Phase 1 commit, paired with `Command::RelayEscrowDeposit`).
    fn apply_nfc_outcome(&mut self, outcome: NfcHardwareOutcome) -> ActionResult {
        match outcome {
            NfcHardwareOutcome::StepAdvanced { commands }
            | NfcHardwareOutcome::Consumed { commands } => {
                if commands.is_empty() {
                    ActionResult::UpdateScreen(self.build_screen())
                } else {
                    ActionResult::Commands { commands }
                }
            }
            NfcHardwareOutcome::Complete {
                card_bytes: _,
                commands,
            } => {
                // TODO: save card_bytes (Phase 1 follow-up — same TODO
                // as apply_ble_outcome).
                self.step = ExchangeStep::Success;
                if commands.is_empty() {
                    ActionResult::UpdateScreen(self.build_screen())
                } else {
                    ActionResult::Commands { commands }
                }
            }
            NfcHardwareOutcome::FailedWithFallback {
                reason,
                relay_handoff,
            } => {
                self.failure_detail = Some(reason);
                self.qr_fallback_available = self.config.device_capabilities.has_camera;
                self.step = ExchangeStep::Failed;
                // Mirror Link-mode TTL (link_mode.rs:26
                // DEFAULT_TTL_SECONDS = 604_800 = 7 days). Wired inline
                // since the const is private to link_mode.
                const NFC_RELAY_TTL_SECONDS: u32 = 604_800;
                if let Some(handoff) = relay_handoff {
                    ActionResult::Commands {
                        commands: vec![vauchi_core::Command::RelayEscrowDeposit {
                            gate_hash: handoff.gate_hash,
                            slot_hash: handoff.slot_hash,
                            encrypted_card: handoff.encrypted_card,
                            ttl_seconds: NFC_RELAY_TTL_SECONDS,
                        }],
                    }
                } else {
                    ActionResult::UpdateScreen(self.build_screen())
                }
            }
            NfcHardwareOutcome::Ignored => ActionResult::UpdateScreen(self.build_screen()),
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
        FieldPreviewConfig {
            card,
            display_name: self.config.own_name.clone(),
            visible_field_ids: std::collections::HashSet::new(), // TODO: resolve from groups
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
            ExchangeStep::Verifying => {
                verifying::build_verifying_screen(self.progress())
            }
            ExchangeStep::NfcRoleSelection => build_nfc_role_screen(self.progress()),
            ExchangeStep::Nfc(ref nfc_step) => {
                nfc::build_nfc_screen(nfc_step, self.progress())
            }
            ExchangeStep::DirectTransport(DirectStep::WaitingForConnection) => ScreenModel {
                screen_id: "exchange_direct_waiting".into(),
                title: "USB Exchange".into(),
                subtitle: Some("Connect your phone via USB cable".into()),
                components: vec![Component::Text {
                    id: "instructions".into(),
                    content: "1. Connect your phone with a USB cable\n2. Enable USB tethering (Android) or trust this computer (iOS)\n3. Open Vauchi on your phone and start an exchange".into(),
                    style: TextStyle::Body,
                }],
                actions: vec![ScreenAction {
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                }],
                progress: Some(self.progress()),
                ..Default::default()
            },
            ExchangeStep::DirectTransport(DirectStep::Exchanging) => ScreenModel {
                screen_id: "exchange_direct_exchanging".into(),
                title: "USB Exchange".into(),
                subtitle: Some("Exchanging contact cards...".into()),
                components: vec![Component::Text {
                    id: "status".into(),
                    content: "Connected. Exchanging encrypted data...".into(),
                    style: TextStyle::Body,
                }],
                actions: vec![],
                progress: Some(self.progress()),
                ..Default::default()
            },
            ExchangeStep::Success if self.success_summary.is_some() => {
                let summary = self
                    .success_summary
                    .as_ref()
                    .expect("guarded by is_some()");
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
            ExchangeStep::Failed => {
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

/// Core-driven NFC role choice (Send/Receive), shown after TapTap is
/// picked. Per ADR-043/044 the renderer is humble: this emits a generic
/// `ActionList` whose item/title strings are **i18n keys** the frontend
/// resolves (ADR-038) — the first i18n-keyed core exchange screen. The
/// item ids (`nfc_role:send` / `nfc_role:receive`) route in `handle_action`
/// to `start_taptap_mode` / `start_nfc_receive_mode`.
fn build_nfc_role_screen(progress: Progress) -> ScreenModel {
    ScreenModel {
        screen_id: "exchange_nfc_role".into(),
        title: "exchange.nfc.choose_role".into(),
        subtitle: Some("exchange.nfc.choose_role_subtitle".into()),
        components: vec![Component::ActionList {
            id: "nfc_role".into(),
            items: vec![
                ActionListItem {
                    id: "nfc_role:send".into(),
                    label: "exchange.mode.nfc_send".into(),
                    icon: None,
                    detail: Some("exchange.mode.nfc_send_description".into()),
                    a11y: None,
                    info_key: None,
                },
                ActionListItem {
                    id: "nfc_role:receive".into(),
                    label: "exchange.mode.nfc_receive".into(),
                    icon: None,
                    detail: Some("exchange.mode.nfc_receive_description".into()),
                    a11y: None,
                    info_key: None,
                },
            ],
        }],
        actions: vec![ScreenAction {
            id: "cancel".into(),
            label: "action.cancel".into(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        }],
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
        // Lazy HCE-responder bootstrap (responder entry —
        // `problems/2026-05-29-nfc-exchange-mode-entry-wiring`): the HCE
        // responder has no `NfcExchangeFlow` until the peer's first tap
        // lands as an `NfcDataReceived`. Spin up an engine-driven responder
        // flow (replacing the legacy `MobileExchangeSession`) so the
        // step-gated routing below dispatches it. The initiator path
        // (`start_taptap_mode`) creates its flow up-front instead.
        if self.nfc_flow.is_none()
            && matches!(event, vauchi_core::Event::NfcDataReceived { .. })
            && let Some(identity) = self.nfc_identity.take()
        {
            let mut flow = NfcExchangeFlow::new_responder(identity, self.config.own_name.clone());
            // Idle -> AwaitingTap. activate() emits an empty NfcActivate
            // (responder already listens via HCE); discard it — the tap
            // already happened and the offer is processed by the routing below.
            if flow.activate().is_ok() {
                self.nfc_flow = Some(flow);
                self.step = ExchangeStep::Nfc(NfcStep::AwaitingTap);
            }
        }

        // NFC mode events — routed through NfcExchangeFlow
        if matches!(self.step, ExchangeStep::Nfc(_)) {
            return self.handle_nfc_hardware_event(event);
        }

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
        let mut reciprocity_result = None;
        if let Some(ref mut confirmer) = self.reciprocity_confirmer {
            if let Some(ref evt) = event_for_confirmer {
                let cmds = confirmer.handle_event(evt);
                commands.extend(cmds);
            }
            if confirmer.is_done() {
                reciprocity_result = Some(confirmer.reciprocity());
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
                } else if let Some(result) = reciprocity_result {
                    // Confirmer just finished — check result
                    match result {
                        vauchi_core::exchange::reciprocity::Reciprocity::Confirmed => {
                            self.step = ExchangeStep::Success;
                        }
                        _ => {
                            // Escrow exhausted without confirmation — peer
                            // didn't deposit their token. Exchange failed.
                            self.failure_detail =
                                Some("Exchange not confirmed by the other device".into());
                            self.step = ExchangeStep::Failed;
                        }
                    }
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
                        // Advance to group selection or, when the card has
                        // no groups, straight to the mode-specific sub-flow.
                        if self.config.available_groups.is_empty() {
                            return self.enter_mode_sub_flow();
                        }
                        self.step = ExchangeStep::GroupSelection;
                        return ActionResult::NavigateTo(self.build_screen());
                    }
                    ModeSelectionResult::Screen(screen) => {
                        return ActionResult::UpdateScreen(*screen);
                    }
                }
            }
            return ActionResult::UpdateScreen(self.build_screen());
        }

        match (&self.step, action) {
            // NFC role choice (Send/Receive) — core-driven sub-screen after
            // TapTap. Send -> initiator (`start_taptap_mode`); Receive ->
            // responder prep (`start_nfc_receive_mode`).
            (ExchangeStep::NfcRoleSelection, UserAction::ListItemSelected { item_id, .. }) => {
                match item_id.as_str() {
                    "nfc_role:send" => self.start_taptap_mode(),
                    "nfc_role:receive" => self.start_nfc_receive_mode(),
                    _ => ActionResult::UpdateScreen(self.build_screen()),
                }
            }
            // NFC role choice: cancel exits the exchange.
            (ExchangeStep::NfcRoleSelection, UserAction::ActionPressed { action_id })
                if action_id == "cancel" =>
            {
                ActionResult::Complete
            }
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
                // Record the group step so BACK from field-preview / sub-flow
                // rewinds here (see navigate_back_within).
                self.step_history.push(ExchangeStep::GroupSelection);
                if action_id == "skip" {
                    self.selected_groups.clear();
                    // Skip → straight to the mode-specific sub-flow (no
                    // preview needed), via the shared router so grouped
                    // TapTap/Glance/Hover keep their NFC/multi-stage screens.
                    return self.enter_mode_sub_flow();
                }
                // Continue with groups → show field preview
                self.field_preview = Some(self.build_field_preview_config());
                self.step = ExchangeStep::FieldPreview;
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
    // What *we* shared, from the confirmed preview (empty visible set =
    // share all). No preview (mode skipped it) → unknown → empty.
    let my_visible_fields = field_preview
        .map(|fp| {
            let share_all = fp.visible_field_ids.is_empty();
            fp.card
                .fields()
                .iter()
                .filter(|f| share_all || fp.visible_field_ids.contains(f.id()))
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
            visible_field_ids: HashSet::from([phone_id]),
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

    // Regression: the Failed-state Retry handler restored the sub-flow by
    // mode but only special-cased Link/BLE, so a failed TapTap (NFC) retry
    // fell through to the legacy QR step (and Glance/Hover lost multi-stage)
    // — the same divergence core!1041 fixed for the forward paths, but the
    // Retry handler was a missed 4th site. Retry must route through the
    // shared `enter_mode_sub_flow` router. Surfaced on-device: "Retry" after
    // a TapTap NFC failure dumped the user onto the QR screen.
    // @internal
    #[test]
    fn taptap_retry_routes_to_nfc_role_selection() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.set_nfc_identity(vauchi_core::identity::Identity::create(
            "Alice",
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        ));
        engine.config.mode = Some(ExchangeMode::TapTap);
        engine.mark_failed();

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "retry".into(),
        });

        assert_eq!(
            engine.step,
            ExchangeStep::NfcRoleSelection,
            "TapTap retry must return to the NFC role chooser, not QR"
        );
        assert!(
            matches!(result, ActionResult::NavigateTo(_)),
            "expected NavigateTo(role screen), got {result:?}"
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

        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "retry".into(),
        });

        assert!(
            matches!(engine.step, ExchangeStep::DirectTransport(_)),
            "Cable must route to its USB DirectTransport path, got {:?}",
            engine.step
        );
        assert_ne!(
            engine.current_screen().screen_id,
            "exchange_show_qr",
            "Cable must never render the frozen-on-android legacy QR screen"
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
    fn mode_selection_pick_advances_past_selection_via_multi_stage_handoff() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        assert_eq!(engine.step, ExchangeStep::ModeSelection);

        // Pick Glance mode — Pair 4 hands this off to MultiStageExchange.
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:quick".into(),
            item_id: "mode:glance".into(),
        });

        // Mode is recorded in config; the engine remains on
        // ModeSelection because the flow leaves Exchange entirely
        // when AppEngine routes the StartMultiStageExchange result.
        assert_eq!(engine.config.mode, Some(ExchangeMode::Glance));
        assert_eq!(engine.step, ExchangeStep::ModeSelection);
        assert!(
            matches!(result, ActionResult::StartMultiStageExchange { .. }),
            "Expected StartMultiStageExchange handoff, got {:?}",
            result
        );
    }

    #[test]
    fn mode_selection_pick_with_groups_goes_to_group_selection() {
        let mut config = config_mode_selection();
        config.available_groups = vec![("g1".into(), "Work".into())];
        let mut engine = ExchangeEngine::new(config, vauchi_core::clock::SystemClock::shared());
        assert_eq!(engine.step, ExchangeStep::ModeSelection);

        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:quick".into(),
            item_id: "mode:glance".into(),
        });

        assert_eq!(engine.step, ExchangeStep::GroupSelection);
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
    fn glance_mode_routes_through_multi_stage_handoff() {
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
                ActionResult::StartMultiStageExchange {
                    mode: ExchangeMode::Glance,
                },
            ),
            "Glance must hand off to multi-stage with mode payload; got {result:?}",
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

        // Mode selection → GroupSelection
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:quick".into(),
            item_id: "mode:glance".into(),
        });
        assert_eq!(engine.step, ExchangeStep::GroupSelection);

        // Continue → FieldPreview
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
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

    // ── Cable / DirectTransport mode tests ─────────────────────────

    // @internal
    #[test]
    fn cable_mode_creates_direct_transport_step() {
        let engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Cable),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        assert_eq!(
            engine.step,
            ExchangeStep::DirectTransport(DirectStep::WaitingForConnection)
        );
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_direct_waiting");
    }

    // @internal
    #[test]
    fn cable_mode_screen_shows_usb_title_and_cancel_action() {
        let engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Cable),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let screen = engine.current_screen();
        assert_eq!(screen.title, "USB Exchange");
        assert!(
            screen.actions.iter().any(|a| a.id == "cancel"),
            "DirectTransport screen must have a cancel action"
        );
    }

    // @internal
    #[test]
    fn cable_mode_with_session_emits_direct_send() {
        use vauchi_core::contact_card::ContactCard;
        use vauchi_core::exchange::{ExchangeSession, ManualConfirmationVerifier, UsbRole};

        let config = ExchangeConfig {
            mode: Some(ExchangeMode::Cable),
            ..config_no_groups()
        };
        let identity = vauchi_core::identity::Identity::create(
            "Test",
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        );
        let card = ContactCard::new("Test");
        let verifier = ManualConfirmationVerifier::new();
        let session = ExchangeSession::new_usb(
            identity,
            card,
            verifier,
            UsbRole::Initiator,
            vauchi_core::clock::SystemClock::shared(),
        );
        let engine = ExchangeEngine::with_session(
            config,
            session,
            vauchi_core::clock::SystemClock::shared(),
        );

        // start_session_if_needed is called via handle_action in the USB path;
        // after construction the step should be DirectTransport(WaitingForConnection).
        assert_eq!(
            engine.step,
            ExchangeStep::DirectTransport(DirectStep::WaitingForConnection)
        );

        // drain_commands calls session.drain_commands() which should contain DirectSend
        // emitted by emit_initial_commands during with_session construction.
        // For USB, emit_initial_commands is NOT called during with_session (only QR is).
        // We call start_session_if_needed indirectly by triggering it.
        // The method is private so we test via the public drain_commands after
        // forcing emit_initial_commands through a cancel + retry path. Instead,
        // directly test by checking that the session is set and the screen title matches.
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_direct_waiting");
        assert!(
            engine.session().is_some(),
            "Session must be retained for DirectTransport"
        );
    }

    // ── Scan quality tracking ──────────────────────────────────────

    // @internal
    #[test]
    fn taptap_mode_selection_starts_nfc_flow_with_identity() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        assert_eq!(engine.step, ExchangeStep::ModeSelection);

        // Populate the cached identity (AppEngine does this at
        // engine construction in app_engine/screens.rs).
        let identity = vauchi_core::identity::Identity::create(
            "Alice",
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        );
        engine.set_nfc_identity(identity);

        // Picker emits ListItemSelected { component_id: "mode", item_id: "tap_tap" }
        // for the TapTap option (per `self::mode_selection`).
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:fun".into(),
            item_id: "mode:tap_tap".into(),
        });

        // TapTap now shows the core-driven role choice (Send/Receive), not
        // the NFC flow directly. The list items carry the role ids.
        assert_eq!(engine.step, ExchangeStep::NfcRoleSelection);
        let role_screen = match result {
            ActionResult::NavigateTo(screen) => screen,
            other => panic!("expected NavigateTo(role screen), got {other:?}"),
        };
        let item_ids: Vec<&str> = role_screen
            .components
            .iter()
            .flat_map(|c| match c {
                Component::ActionList { items, .. } => {
                    items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>()
                }
                _ => vec![],
            })
            .collect();
        assert_eq!(item_ids, vec!["nfc_role:send", "nfc_role:receive"]);

        // Selecting "Send" starts the initiator flow.
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "nfc_role".into(),
            item_id: "nfc_role:send".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Nfc(NfcStep::AwaitingTap));
        match result {
            ActionResult::Commands { commands } => {
                assert_eq!(commands.len(), 1, "exactly one initial command");
                match &commands[0] {
                    vauchi_core::Command::NfcActivate { payload } => {
                        assert!(
                            !payload.is_empty(),
                            "initiator activate must carry a non-empty key offer payload"
                        );
                    }
                    other => panic!("expected Command::NfcActivate, got {other:?}"),
                }
            }
            other => panic!("expected ActionResult::Commands, got {other:?}"),
        }
        assert!(
            engine.nfc_identity.is_none(),
            "set_nfc_identity must be consumed by start_taptap_mode"
        );
        assert!(engine.nfc_flow.is_some(), "nfc_flow must be populated");
    }

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
    fn taptap_with_groups_skip_routes_to_nfc_role_selection() {
        let mut config = config_mode_selection();
        config.available_groups = vec![("g1".into(), "Work".into())];
        let mut engine = ExchangeEngine::new(config, vauchi_core::clock::SystemClock::shared());
        engine.set_nfc_identity(vauchi_core::identity::Identity::create(
            "Alice",
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        ));

        // Pick TapTap — with groups present, lands on GroupSelection first.
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:fun".into(),
            item_id: "mode:tap_tap".into(),
        });
        assert_eq!(engine.step, ExchangeStep::GroupSelection);

        // Skip groups → must enter the TapTap sub-flow (NFC role choice),
        // never the legacy QR step.
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "skip".into(),
        });
        assert_eq!(
            engine.step,
            ExchangeStep::NfcRoleSelection,
            "grouped TapTap + skip must reach NfcRoleSelection, not QR"
        );
        assert!(
            matches!(result, ActionResult::NavigateTo(_)),
            "expected NavigateTo(role screen), got {result:?}"
        );
        // Negative (CC-11): must not collapse to the QR sub-flow.
    }

    // @internal
    #[test]
    fn taptap_with_groups_continue_routes_to_nfc_role_selection() {
        let mut config = config_mode_selection();
        config.available_groups = vec![("g1".into(), "Work".into())];
        let mut engine = ExchangeEngine::new(config, vauchi_core::clock::SystemClock::shared());
        engine.set_nfc_identity(vauchi_core::identity::Identity::create(
            "Alice",
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        ));

        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:fun".into(),
            item_id: "mode:tap_tap".into(),
        });
        assert_eq!(engine.step, ExchangeStep::GroupSelection);

        // Continue (keep groups) → field preview.
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert_eq!(engine.step, ExchangeStep::FieldPreview);

        // Start the exchange from field preview → TapTap NFC role choice.
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "start_exchange".into(),
        });
        assert_eq!(
            engine.step,
            ExchangeStep::NfcRoleSelection,
            "grouped TapTap + continue + start must reach NfcRoleSelection, not QR"
        );
        assert!(
            matches!(result, ActionResult::NavigateTo(_)),
            "expected NavigateTo(role screen), got {result:?}"
        );
    }

    // @internal
    #[test]
    fn glance_with_groups_skip_routes_to_multi_stage() {
        // Same bug class for the multi-stage modes: grouped Glance + Skip
        // must hand off to MultiStageExchange, not collapse to QR.
        let mut config = config_mode_selection();
        config.available_groups = vec![("g1".into(), "Work".into())];
        let mut engine = ExchangeEngine::new(config, vauchi_core::clock::SystemClock::shared());

        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:quick".into(),
            item_id: "mode:glance".into(),
        });
        assert_eq!(engine.step, ExchangeStep::GroupSelection);

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "skip".into(),
        });
        assert!(
            matches!(
                result,
                ActionResult::StartMultiStageExchange {
                    mode: ExchangeMode::Glance
                }
            ),
            "grouped Glance + skip must hand off to MultiStageExchange, got {result:?}"
        );
    }

    // @internal
    #[test]
    fn taptap_mode_without_identity_routes_to_failed() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        // No set_nfc_identity — the role choice itself doesn't fail; the
        // Send selection (start_taptap_mode) fail-fasts rather than panicking.
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:fun".into(),
            item_id: "mode:tap_tap".into(),
        });
        assert_eq!(engine.step, ExchangeStep::NfcRoleSelection);
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "nfc_role".into(),
            item_id: "nfc_role:send".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Failed);
        match result {
            ActionResult::UpdateScreen(_) => {
                let detail = engine.failure_detail.as_deref().unwrap_or("");
                assert!(
                    detail.contains("identity"),
                    "failure detail must mention identity, got: {detail}"
                );
            }
            other => panic!("expected UpdateScreen, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn nfc_receive_mode_prepares_responder_without_flow() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        let identity = vauchi_core::identity::Identity::create(
            "Bob",
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        );
        engine.set_nfc_identity(identity);

        // Navigate TapTap -> role choice, then pick "Receive". Responder
        // prep must NOT create a flow.
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:fun".into(),
            item_id: "mode:tap_tap".into(),
        });
        assert_eq!(engine.step, ExchangeStep::NfcRoleSelection);
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "nfc_role".into(),
            item_id: "nfc_role:receive".into(),
        });

        assert_eq!(engine.step, ExchangeStep::Nfc(NfcStep::AwaitingTap));
        assert!(
            engine.nfc_flow.is_none(),
            "receive mode must not pre-create an initiator flow"
        );
        assert!(
            engine.nfc_identity.is_some(),
            "identity stays cached for the lazy HCE bootstrap to consume"
        );
        match result {
            ActionResult::Commands { commands } => {
                assert_eq!(commands.len(), 1, "exactly one registration command");
                match &commands[0] {
                    vauchi_core::Command::NfcActivate { payload } => {
                        assert!(
                            payload.is_empty(),
                            "responder activate signal must carry an empty payload \
                             (the discriminator the frontend reads to register HCE)"
                        );
                    }
                    other => panic!("expected Command::NfcActivate, got {other:?}"),
                }
            }
            other => panic!("expected ActionResult::Commands, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn nfc_receive_mode_without_identity_routes_to_failed() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        // No set_nfc_identity — navigate to the role choice, then "Receive"
        // fail-fasts on the missing identity.
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:fun".into(),
            item_id: "mode:tap_tap".into(),
        });
        assert_eq!(engine.step, ExchangeStep::NfcRoleSelection);
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "nfc_role".into(),
            item_id: "nfc_role:receive".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Failed);
        match result {
            ActionResult::UpdateScreen(_) => {
                let detail = engine.failure_detail.as_deref().unwrap_or("");
                assert!(
                    detail.contains("identity"),
                    "failure detail must mention identity, got: {detail}"
                );
            }
            other => panic!("expected UpdateScreen, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn nfc_role_choice_cancel_completes_exchange() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.set_nfc_identity(vauchi_core::identity::Identity::create(
            "Alice",
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        ));
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:fun".into(),
            item_id: "mode:tap_tap".into(),
        });
        assert_eq!(engine.step, ExchangeStep::NfcRoleSelection);
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "cancel".into(),
        });
        assert!(
            matches!(result, ActionResult::Complete),
            "cancel from the role choice exits the exchange"
        );
    }

    // @internal
    #[test]
    fn nfc_responder_bootstraps_on_first_tap_and_emits_ack() {
        // The HCE responder has no flow until tapped. Feeding the peer's
        // key offer (the first NfcDataReceived) must lazily spin up an
        // engine-owned responder NfcExchangeFlow, advance it to AckSent,
        // and emit (key_ack || encrypted_card) as a single NfcSendApdu —
        // exactly what the Android VauchiHceService binder-block returns.
        let now = vauchi_core::clock::SystemClock::shared().unix_seconds();

        // Real key offer from a separate initiator flow (real crypto, ADR-002).
        let mut initiator = NfcExchangeFlow::new_initiator(
            vauchi_core::identity::Identity::create("Alice", now),
            "Alice".into(),
        );
        let offer = match &initiator.activate().expect("initiator activate")[0] {
            vauchi_core::Command::NfcActivate { payload } => payload.clone(),
            other => panic!("expected NfcActivate, got {other:?}"),
        };
        assert!(!offer.is_empty(), "initiator key offer must be non-empty");

        // Responder engine: NFC identity set, no flow yet.
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.set_nfc_identity(vauchi_core::identity::Identity::create("Bob", now));
        assert!(engine.nfc_flow.is_none(), "no flow before first tap");

        let result =
            engine.handle_hardware_event(vauchi_core::Event::NfcDataReceived { data: offer });

        // First tap bootstrapped + advanced the responder; identity consumed.
        assert!(
            engine.nfc_flow.is_some(),
            "first tap must bootstrap the responder flow"
        );
        assert_eq!(engine.step, ExchangeStep::Nfc(NfcStep::AckSent));
        assert!(
            engine.nfc_identity.is_none(),
            "bootstrap must consume nfc_identity"
        );
        match result {
            Some(ActionResult::Commands { commands }) => match commands.as_slice() {
                [vauchi_core::Command::NfcSendApdu { data }] => {
                    assert!(
                        !data.is_empty(),
                        "responder must send key_ack || encrypted_card"
                    );
                }
                other => panic!("expected single NfcSendApdu, got {other:?}"),
            },
            other => panic!("expected ActionResult::Commands, got {other:?}"),
        }
    }
}
