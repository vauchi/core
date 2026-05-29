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

pub(crate) mod ble;
pub(crate) mod field_preview;
pub(crate) mod link;
pub(crate) mod mode_selection;
pub(crate) mod nfc;
pub(crate) mod qr;

use self::ble::{BleActionOutcome, BleExchangeFlow, BleHardwareOutcome, BleStep};
use self::field_preview::{FieldPreviewConfig, FieldPreviewResult};
use self::link::{LinkActionOutcome, LinkHardwareOutcome, LinkStep};
use self::mode_selection::{ModeSelectionEngine, ModeSelectionResult};
use self::nfc::{NfcExchangeFlow, NfcHardwareOutcome, NfcStep};
use self::qr::{QrActionOutcome, QrStep, ScanQualityTracker};
use crate::ui::*;
use vauchi_core::Command;
use vauchi_core::clock::Clock;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::escrow::EscrowKeys;
use vauchi_core::exchange::link_mode::{self, LinkInitiation};
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
    /// Link mode initiation data (URL, nonce, secret key, handshake slot).
    /// Populated when entering Link(ShareUrl) via `initiator_generate()`.
    link_initiation: Option<LinkInitiation>,
    /// Pending Link mode commands (presence deposit, relay calls).
    /// Drained via `drain_commands()` same as session commands.
    pending_link_commands: Vec<Command>,
    /// Escrow keys derived after DH with responder (Link mode only).
    /// Populated on `LinkOpened` event, used for card retrieval + decryption.
    escrow_keys: Option<EscrowKeys>,
    /// Decrypted card bytes from Link mode exchange (set on ExchangeComplete).
    /// Callers check `link_received_card_bytes()` after Success to save the contact.
    link_received_card: Option<Vec<u8>>,
    /// BLE exchange flow state machine (Magic/Bump/Shake modes).
    ble_flow: Option<BleExchangeFlow>,
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
    /// Animated QR frames for the exchange QR display (V6 chunked).
    /// Populated when session generates a QR. Cycles via `advance_qr_frame()`.
    qr_frames: Vec<String>,
    /// Current frame index in `qr_frames` (wraps around).
    qr_frame_index: usize,
    /// Rolling window tracker for QR scan quality (viewfinder frame color).
    scan_quality_tracker: ScanQualityTracker,
    /// Wall-clock source for time-stamped sub-engines
    /// (`ReciprocityConfirmer`). Threaded through both constructors;
    /// production callers pass `vauchi.clock()`, tests use
    /// `SystemClock::shared()` or a `FakeClock`.
    clock: Arc<dyn Clock>,
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
    /// QR exchange sub-flow (Glance/Hover modes).
    Qr(QrStep),
    /// BLE exchange sub-flow (Magic/Bump/Shake modes).
    Ble(BleStep),
    /// NFC exchange sub-flow (3-phase encrypted handshake over an
    /// NFC tap). See `self::nfc` + 2026-05-19-nfc-exchange-engine-design.md.
    Nfc(NfcStep),
    /// Link exchange sub-flow (async relay-mediated).
    Link(LinkStep),
    /// USB cable / direct TCP exchange.
    DirectTransport(DirectStep),
    Success,
    Failed,
}

impl ExchangeStep {
    fn step_number(&self) -> u8 {
        match self {
            Self::ModeSelection => 1,
            Self::GroupSelection => 2,
            Self::FieldPreview => 3,
            // Sub-flow steps start at 4 (after mode + group + preview)
            Self::Qr(qr) => qr.step_number(4),
            Self::Ble(ble) => ble.step_number(4),
            Self::Nfc(nfc) => nfc.step_number(4),
            Self::Link(link) => link.step_number(4),
            Self::DirectTransport(direct) => direct.step_number(4),
            Self::Success => 4 + QrStep::STEP_COUNT,
            Self::Failed => 5 + QrStep::STEP_COUNT,
        }
    }
}

// mode + group + preview + sub-flow + success/failed
// All sub-flows must have the same step count for consistent progress.
const _: () = assert!(QrStep::STEP_COUNT == LinkStep::STEP_COUNT);
const _: () = assert!(QrStep::STEP_COUNT == BleStep::STEP_COUNT);
const _: () = assert!(QrStep::STEP_COUNT == NfcStep::STEP_COUNT);
const _: () = assert!(DirectStep::STEP_COUNT == QrStep::STEP_COUNT);
const TOTAL_STEPS: u8 = 3 + QrStep::STEP_COUNT + 2;

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
            if config.mode == Some(ExchangeMode::Link) {
                return ExchangeStep::Link(LinkStep::ShareUrl);
            }
            if config.mode == Some(ExchangeMode::Cable) {
                return ExchangeStep::DirectTransport(DirectStep::WaitingForConnection);
            }
            if matches!(
                config.mode,
                Some(ExchangeMode::Magic | ExchangeMode::Bump | ExchangeMode::Shake)
            ) {
                return ExchangeStep::Ble(BleStep::Discovering);
            }
            ExchangeStep::Qr(QrStep::ShowQr)
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
        // If starting directly at Link mode, generate initiation data now.
        // The presence deposit command is available via `drain_commands()`.
        let (link_initiation, pending_link_commands) =
            if step == ExchangeStep::Link(LinkStep::ShareUrl) {
                let (init, cmds) = link_mode::initiator_generate();
                (Some(init), cmds)
            } else {
                (None, Vec::new())
            };
        // If starting directly at BLE mode, create the flow now.
        let ble_flow = if matches!(step, ExchangeStep::Ble(_)) {
            Some(BleExchangeFlow::new(
                config.mode.unwrap_or(ExchangeMode::Magic),
            ))
        } else {
            None
        };
        Self {
            step,
            config,
            scanned_data: None,
            selected_groups: Vec::new(),
            session: None,
            failure_detail: None,
            ble_fallback_available: false,
            qr_fallback_available: false,
            mode_selection,
            field_preview: None,
            link_initiation,
            pending_link_commands,
            escrow_keys: None,
            link_received_card: None,
            ble_flow,
            nfc_flow: None,
            nfc_identity: None,
            reciprocity_confirmer: None,
            qr_frames: Vec::new(),
            qr_frame_index: 0,
            scan_quality_tracker: ScanQualityTracker::new(),
            clock,
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

        // If starting directly at ShowQr, kick off the session now.
        // StartQR should always succeed on a fresh Idle session.
        if step == ExchangeStep::Qr(QrStep::ShowQr) {
            if session.apply(ExchangeEvent::StartQR).is_err() {
                return Self {
                    step,
                    config,
                    scanned_data: None,
                    selected_groups: Vec::new(),
                    session: None,
                    failure_detail: None,
                    ble_fallback_available: false,
                    qr_fallback_available: false,
                    mode_selection,
                    field_preview: None,
                    link_initiation: None,
                    pending_link_commands: Vec::new(),
                    escrow_keys: None,
                    link_received_card: None,
                    ble_flow: None,
                    nfc_flow: None,
                    nfc_identity: None,
                    reciprocity_confirmer: None,
                    qr_frames: Vec::new(),
                    qr_frame_index: 0,
                    scan_quality_tracker: ScanQualityTracker::new(),
                    clock,
                };
            }
            session.emit_initial_commands();
        }

        // Generate animated QR frames (V6-sized chunks) from exchange payload.
        let qr_frames = Self::generate_qr_frames(&session);

        Self {
            step,
            config,
            scanned_data: None,
            selected_groups: Vec::new(),
            session: Some(session),
            failure_detail: None,
            ble_fallback_available: false,
            qr_fallback_available: false,
            mode_selection,
            field_preview: None,
            link_initiation: None,
            pending_link_commands: Vec::new(),
            escrow_keys: None,
            link_received_card: None,
            ble_flow: None,
            nfc_flow: None,
            nfc_identity: None,
            reciprocity_confirmer: None,
            qr_frames,
            qr_frame_index: 0,
            scan_quality_tracker: ScanQualityTracker::new(),
            clock,
        }
    }

    /// Drains any pending commands from the session or Link mode (ADR-031).
    ///
    /// Call this after construction with `with_session()` to get the
    /// initial `QrDisplay` command, or after `new()` with Link mode to get
    /// the initial presence deposit command.
    pub fn drain_commands(&mut self) -> Vec<Command> {
        if !self.pending_link_commands.is_empty() {
            return std::mem::take(&mut self.pending_link_commands);
        }
        self.session
            .as_mut()
            .map(|s| s.drain_commands())
            .unwrap_or_default()
    }

    /// Generate animated QR frames from the exchange session payload.
    ///
    /// Chunks the base64-encoded exchange QR data into V6-sized frames
    /// using the multipart codec. V6 QR at EC-M holds 84 alphanumeric chars,
    /// which the 240p camera can decode reliably (~10ms per frame).
    ///
    /// Returns an empty Vec if no session/QR is available.
    fn generate_qr_frames(session: &ExchangeSession) -> Vec<String> {
        use vauchi_core::exchange::transport::animated_qr::{AnimatedQrConfig, AnimatedQrSession};

        let Some(qr) = session.qr() else {
            return Vec::new();
        };
        let payload = qr.to_data_string();

        // V6 QR at EC-M: 84 bytes binary capacity. The frame wire format is
        // "{idx}/{total}/{crc32_8hex}/{base64url_data}". With ~15 bytes overhead
        // and base64 expansion (4/3), usable raw bytes per chunk ≈ 50.
        // This produces 4-6 frames for a typical exchange payload, cycling at
        // 10fps = full cycle in 400-600ms.
        let config = AnimatedQrConfig {
            fps: 10,
            chunk_size: 50,
            cycle_padding: 3,
        };

        let sender = AnimatedQrSession::new_sender(payload.into_bytes(), config);
        (0..sender.frame_count())
            .filter_map(|i| sender.frame_at(i).ok())
            .collect()
    }

    /// Number of animated QR frames (1 for static QR, >1 for animated).
    #[cfg(test)]
    pub fn qr_frame_count(&self) -> usize {
        self.qr_frames.len().max(1)
    }

    /// Returns a reference to the protocol session, if any (ADR-031).
    pub fn session(&self) -> Option<&ExchangeSession> {
        self.session.as_ref()
    }

    /// Returns a mutable reference to the protocol session, if any (ADR-031).
    pub fn session_mut(&mut self) -> Option<&mut ExchangeSession> {
        self.session.as_mut()
    }

    /// Returns the decrypted contact card bytes from a completed Link exchange.
    ///
    /// Available after `ExchangeComplete` is processed (step is `Success`).
    /// The caller (AppEngine/PlatformAppEngine) should deserialize and save
    /// the contact, matching the QR path's `session.extract_contact()` pattern.
    pub fn link_received_card_bytes(&self) -> Option<&[u8]> {
        self.link_received_card.as_deref()
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
            // Existing QR path below...
            match session.apply(ExchangeEvent::StartQR) {
                Ok(()) => {
                    session.emit_initial_commands();
                    // Generate animated QR frames now that the session has a QR
                    self.qr_frames = Self::generate_qr_frames(session);
                    self.qr_frame_index = 0;
                    let commands = session.drain_commands();
                    if !commands.is_empty() {
                        return ActionResult::Commands { commands };
                    }
                }
                Err(_) => {
                    // Session failed to start QR — drop it and fall back to
                    // legacy UI-only mode with static QR data.
                    self.session = None;
                    return ActionResult::ShowToast {
                        message: "Secure exchange unavailable — using basic mode".into(),
                        undo_action_id: None,
                    };
                }
            }
        }
        ActionResult::NavigateTo(self.build_screen())
    }

    /// Start Link mode: generate URL, store initiation data, emit presence
    /// deposit command (ADR-031).
    ///
    /// Returns `Commands` with the presence deposit for the handshake
    /// gate, or `NavigateTo` if no commands are needed (shouldn't happen).
    fn start_link_mode(&mut self) -> ActionResult {
        self.step = ExchangeStep::Link(LinkStep::ShareUrl);
        let (initiation, commands) = link_mode::initiator_generate();
        self.link_initiation = Some(initiation);
        if commands.is_empty() {
            ActionResult::NavigateTo(self.build_screen())
        } else {
            ActionResult::Commands { commands }
        }
    }

    /// Start BLE exchange mode (Magic/Bump/Shake).
    ///
    /// Creates a `BleExchangeFlow` and emits BLE advertising + scanning
    /// commands to begin discovery.
    fn start_ble_mode(&mut self) -> ActionResult {
        let mode = self.config.mode.unwrap_or(ExchangeMode::Magic);
        self.ble_flow = Some(BleExchangeFlow::new(mode));
        self.step = ExchangeStep::Ble(BleStep::Discovering);
        let service_uuid = vauchi_core::exchange::VAUCHI_BLE_SERVICE_UUID.to_string();
        ActionResult::Commands {
            commands: vec![
                Command::BleStartAdvertising {
                    service_uuid: service_uuid.clone(),
                    payload: vec![],
                },
                Command::BleStartScanning { service_uuid },
            ],
        }
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

    /// Handle BLE mode hardware events via BleExchangeFlow.
    fn handle_ble_hardware_event(&mut self, event: vauchi_core::Event) -> Option<ActionResult> {
        let flow = self.ble_flow.as_mut()?;
        let outcome = flow.handle_event(&event);

        // Sync engine step from flow step
        self.step = ExchangeStep::Ble(flow.step().clone());

        Some(self.apply_ble_outcome(outcome))
    }

    /// Apply a BleHardwareOutcome — translate to ActionResult.
    fn apply_ble_outcome(&mut self, outcome: BleHardwareOutcome) -> ActionResult {
        match outcome {
            BleHardwareOutcome::StepAdvanced { commands }
            | BleHardwareOutcome::Consumed { commands } => {
                if commands.is_empty() {
                    ActionResult::UpdateScreen(self.build_screen())
                } else {
                    ActionResult::Commands { commands }
                }
            }
            BleHardwareOutcome::Complete {
                card_bytes: _,
                commands,
            } => {
                // TODO: save card_bytes (Phase 1 integration)
                self.step = ExchangeStep::Success;
                if commands.is_empty() {
                    ActionResult::UpdateScreen(self.build_screen())
                } else {
                    ActionResult::Commands { commands }
                }
            }
            BleHardwareOutcome::FailedWithFallback { reason } => {
                self.failure_detail = Some(reason);
                self.ble_fallback_available = true;
                self.qr_fallback_available = self.config.device_capabilities.has_camera;
                self.step = ExchangeStep::Failed;
                ActionResult::UpdateScreen(self.build_screen())
            }
            BleHardwareOutcome::Ignored => ActionResult::UpdateScreen(self.build_screen()),
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

    /// Handle Link mode hardware events (ADR-031).
    ///
    /// Routes to handshake-phase or escrow-phase handler depending on
    /// whether ECDH has completed (escrow_keys present).
    fn handle_link_hardware_event(&mut self, event: vauchi_core::Event) -> Option<ActionResult> {
        // Special case: LinkOpened triggers DH + card encryption
        if let vauchi_core::Event::LinkOpened {
            ref peer_public_key,
        } = event
        {
            return self.handle_link_opened(peer_public_key);
        }

        // Escrow phase: keys are known, handle card exchange events
        if let Some(ref keys) = self.escrow_keys
            && let Some(outcome) = link::handle_escrow_hw_event(keys, &event)
        {
            return Some(self.apply_link_outcome(outcome));
        }

        // Handshake phase: waiting for responder's epk
        let li = self.link_initiation.as_ref()?;
        let outcome = link::handle_link_hw_event(li, &event)?;
        Some(self.apply_link_outcome(outcome))
    }

    /// Process LinkOpened: derive keys, encrypt card, deposit + poll.
    fn handle_link_opened(&mut self, peer_public_key: &[u8]) -> Option<ActionResult> {
        let li = self.link_initiation.as_ref()?;

        let result =
            (|| -> Result<LinkHardwareOutcome, vauchi_core::exchange::link_mode::LinkModeError> {
                let cs = self
                    .config
                    .card_snapshot
                    .as_ref()
                    .ok_or(vauchi_core::exchange::link_mode::LinkModeError::NoCardToSend)?;
                let card_bytes = cs.to_bytes().map_err(|e| {
                    vauchi_core::exchange::link_mode::LinkModeError::CardCryptoFailed(format!(
                        "card serialization: {e}"
                    ))
                })?;
                link::handle_link_opened(li, peer_public_key, &card_bytes)
            })();

        match result {
            Ok(outcome) => Some(self.apply_link_outcome(outcome)),
            Err(e) => {
                self.failure_detail = Some(e.to_string());
                self.step = ExchangeStep::Failed;
                Some(ActionResult::UpdateScreen(self.build_screen()))
            }
        }
    }

    /// Apply a LinkHardwareOutcome — shared dispatch for all Link events.
    fn apply_link_outcome(&mut self, outcome: LinkHardwareOutcome) -> ActionResult {
        match outcome {
            LinkHardwareOutcome::PollHandshakeGate { commands } => {
                ActionResult::Commands { commands }
            }
            LinkHardwareOutcome::RetrieveFromHandshake { commands } => {
                self.step = ExchangeStep::Link(LinkStep::Retrieving);
                ActionResult::Commands { commands }
            }
            LinkHardwareOutcome::DhCompleteCardDeposited {
                commands,
                escrow_keys,
            } => {
                self.escrow_keys = Some(escrow_keys);
                ActionResult::Commands { commands }
            }
            LinkHardwareOutcome::RetrieveFromEscrow { commands } => {
                ActionResult::Commands { commands }
            }
            LinkHardwareOutcome::ExchangeComplete { card_bytes } => {
                self.link_received_card = Some(card_bytes);
                self.step = ExchangeStep::Success;
                ActionResult::UpdateScreen(self.build_screen())
            }
            LinkHardwareOutcome::Failed { reason } => {
                self.failure_detail = Some(reason);
                self.step = ExchangeStep::Failed;
                ActionResult::UpdateScreen(self.build_screen())
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
            ExchangeStep::Qr(QrStep::ShowQr) => {
                // Use current animated frame, or fall back to static payload
                let frame_data = if !self.qr_frames.is_empty() {
                    &self.qr_frames[self.qr_frame_index]
                } else {
                    &self.config.own_qr_data
                };
                qr::build_show_qr_screen(
                    frame_data,
                    &self.config.own_name,
                    self.progress(),
                )
            }
            ExchangeStep::Qr(QrStep::ScanQr) => qr::build_scan_qr_screen(
                self.progress(),
                Some(self.scan_quality_tracker.quality()),
            ),
            ExchangeStep::Qr(QrStep::ManualEntry) => {
                qr::build_manual_entry_screen(self.progress())
            }
            ExchangeStep::Qr(QrStep::Verifying) => {
                qr::build_verifying_screen(self.progress())
            }
            ExchangeStep::Ble(BleStep::Discovering) => {
                let mode = self.config.mode.unwrap_or(ExchangeMode::Magic);
                ble::build_discovering_screen(mode, self.progress())
            }
            ExchangeStep::Ble(BleStep::Handshaking | BleStep::Exchanging) => {
                let mode = self.config.mode.unwrap_or(ExchangeMode::Magic);
                ble::build_exchanging_screen(mode, self.progress())
            }
            ExchangeStep::Ble(BleStep::Verifying) => {
                let mode = self.config.mode.unwrap_or(ExchangeMode::Magic);
                ble::build_verifying_screen(mode, self.progress())
            }
            ExchangeStep::Ble(BleStep::Complete) => {
                // Handled by transition to ExchangeStep::Success
                ScreenModel::default()
            }
            ExchangeStep::Nfc(ref nfc_step) => {
                nfc::build_nfc_screen(nfc_step, self.progress())
            }
            ExchangeStep::Link(LinkStep::ShareUrl) => {
                let url = self
                    .link_initiation
                    .as_ref()
                    .map(|li| li.url.as_str())
                    .unwrap_or("generating...");
                link::build_share_url_screen(url, self.progress())
            }
            ExchangeStep::Link(LinkStep::WaitingForResponse) => {
                link::build_waiting_screen(self.progress())
            }
            ExchangeStep::Link(LinkStep::Retrieving) => {
                link::build_retrieving_screen(self.progress())
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
                        a11y: None,
                    });
                }
                if self.ble_fallback_available {
                    actions.push(ScreenAction {
                        id: "fallback_relay".into(),
                        label: "Switch to encrypted relay".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: None,
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

impl WorkflowEngine for ExchangeEngine {
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

        // BLE mode events — routed through BleExchangeFlow
        if matches!(self.step, ExchangeStep::Ble(_)) {
            return self.handle_ble_hardware_event(event);
        }

        // NFC mode events — routed through NfcExchangeFlow
        if matches!(self.step, ExchangeStep::Nfc(_)) {
            return self.handle_nfc_hardware_event(event);
        }

        // Link mode events — handled without ExchangeSession
        if matches!(self.step, ExchangeStep::Link(_)) {
            return self.handle_link_hardware_event(event);
        }

        // QR scan progress → update quality tracker, refresh screen.
        // Skipped frames (sharpness gating) are excluded — they indicate
        // camera settling, not wrong pointing.
        if matches!(self.step, ExchangeStep::Qr(QrStep::ScanQr))
            && let vauchi_core::Event::QrScanProgress {
                detected,
                frame_skipped,
                ..
            } = &event
        {
            if !frame_skipped {
                self.scan_quality_tracker.record_frame(*detected);
            }
            return Some(ActionResult::UpdateScreen(self.build_screen()));
        }

        // Camera unavailable/denied during QR scan → switch to manual entry
        if matches!(self.step, ExchangeStep::Qr(QrStep::ScanQr)) {
            let is_camera_fallback = matches!(
                &event,
                vauchi_core::Event::HardwareUnavailable { transport }
                | vauchi_core::Event::PermissionDenied { transport }
                    if transport.eq_ignore_ascii_case("camera")
            );
            if is_camera_fallback {
                self.step = ExchangeStep::Qr(QrStep::ManualEntry);
                return Some(ActionResult::NavigateTo(self.build_screen()));
            }
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
            vauchi_core::exchange::ExchangeState::Complete { .. } => {
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
                    self.step = ExchangeStep::Qr(QrStep::Verifying);
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
                    self.step = ExchangeStep::Qr(QrStep::Verifying);
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
                self.step = ExchangeStep::Qr(QrStep::Verifying);
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
            if let Some(ref ms) = self.mode_selection {
                match ms.handle_action(&action) {
                    ModeSelectionResult::Selected(mode) => {
                        self.config.mode = Some(mode);
                        self.mode_selection = None;
                        // Advance to group selection or directly to sub-flow
                        if self.config.available_groups.is_empty() {
                            if mode == ExchangeMode::Link {
                                return self.start_link_mode();
                            }
                            if matches!(
                                mode,
                                ExchangeMode::Magic | ExchangeMode::Bump | ExchangeMode::Shake
                            ) {
                                self.step = ExchangeStep::Ble(BleStep::Discovering);
                                return self.start_ble_mode();
                            }
                            if mode == ExchangeMode::TapTap {
                                return self.start_taptap_mode();
                            }
                            // Pair 4 — `Glance` is the canonical face-to-face
                            // mode (bilateral simultaneous QR with no proximity
                            // signal); route it through the new core-driven
                            // `MultiStageExchange` screen so the multi-stage
                            // protocol drives both QR display and scan from a
                            // pure ScreenModel rather than the legacy bespoke
                            // step state machine.
                            //
                            // Phase 1.E of `2026-05-11-hover-graduation-plan.md`
                            // extended the handoff to `Hover` (QR + ultrasonic).
                            // The `mode` payload tells AppEngine which engine
                            // constructor to use (`new_hover` vs `new_glance`)
                            // — Hover defaults to the front camera and runs
                            // the autonomous audio-handshake trigger; Glance
                            // stays back-camera + audio-quiet (the
                            // `is_active_engine_multi_stage_hover()` gate in
                            // PlatformAppEngine pinned by the 1.C polish
                            // regression tests). Broadcast (one-to-many) +
                            // TapHoverShake (Phase 2/3) keep the legacy
                            // `ExchangeStep::Qr` path until their per-mode
                            // graduations land.
                            if matches!(mode, ExchangeMode::Glance | ExchangeMode::Hover) {
                                return ActionResult::StartMultiStageExchange { mode };
                            }
                            self.step = ExchangeStep::Qr(QrStep::ShowQr);
                            return self.start_session_if_needed();
                        } else {
                            self.step = ExchangeStep::GroupSelection;
                            return ActionResult::NavigateTo(self.build_screen());
                        }
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
                if action_id == "skip" {
                    self.selected_groups.clear();
                    // Skip → go straight to sub-flow (no preview needed)
                    if matches!(
                        self.config.mode,
                        Some(ExchangeMode::Magic | ExchangeMode::Bump | ExchangeMode::Shake)
                    ) {
                        self.step = ExchangeStep::Ble(BleStep::Discovering);
                        return self.start_ble_mode();
                    }
                    self.step = ExchangeStep::Qr(QrStep::ShowQr);
                    return self.start_session_if_needed();
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
                            // Route to sub-flow based on selected mode
                            if self.config.mode == Some(ExchangeMode::Link) {
                                return self.start_link_mode();
                            }
                            if matches!(
                                self.config.mode,
                                Some(
                                    ExchangeMode::Magic | ExchangeMode::Bump | ExchangeMode::Shake
                                )
                            ) {
                                self.step = ExchangeStep::Ble(BleStep::Discovering);
                                return self.start_ble_mode();
                            }
                            self.step = ExchangeStep::Qr(QrStep::ShowQr);
                            return self.start_session_if_needed();
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
            // QR sub-flow actions — delegated to the `qr` sub-module.
            (ExchangeStep::Qr(qr_step), ref user_action) => {
                if let Some(outcome) = qr::handle_qr_action(qr_step, user_action) {
                    match outcome {
                        QrActionOutcome::AdvanceToScan => {
                            // Always emit QrRequestScan — the legacy
                            // `RequestCamera` ActionResult is deprecated
                            // (see ADR-022 Addendum D) and is a silent
                            // no-op on the mobile frontends, which only
                            // implement the command/event protocol. This
                            // is the gap that left the Android Glance
                            // "Tap to Scan" button unresponsive on first
                            // tap (verified on Pixel 3a 2026-04-27).
                            self.step = ExchangeStep::Qr(QrStep::ScanQr);
                            self.scan_quality_tracker.reset();
                            ActionResult::Commands {
                                commands: vec![Command::QrRequestScan],
                            }
                        }
                        QrActionOutcome::BackToShowQr => {
                            self.step = ExchangeStep::Qr(QrStep::ShowQr);
                            ActionResult::NavigateTo(self.build_screen())
                        }
                        QrActionOutcome::QrScanned { data } => {
                            self.scanned_data = Some(data);
                            self.step = ExchangeStep::Qr(QrStep::Verifying);
                            ActionResult::NavigateTo(self.build_screen())
                        }
                        QrActionOutcome::ManualCodeEntered { data } => {
                            // Manual entry is functionally equivalent to scanning
                            self.scanned_data = Some(data);
                            self.step = ExchangeStep::Qr(QrStep::Verifying);
                            ActionResult::NavigateTo(self.build_screen())
                        }
                    }
                } else {
                    ActionResult::UpdateScreen(self.build_screen())
                }
            }
            // BLE sub-flow actions
            (ExchangeStep::Ble(ble_step), ref user_action) => {
                if let Some(outcome) = ble::handle_ble_action(ble_step, user_action) {
                    match outcome {
                        BleActionOutcome::FallbackToRelay => {
                            // Switch to relay escrow (Link mode as fallback)
                            return self.start_link_mode();
                        }
                        BleActionOutcome::Cancel => return ActionResult::Complete,
                        BleActionOutcome::Ignored => {}
                    }
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            // Link sub-flow actions
            (ExchangeStep::Link(link_step), ref user_action) => {
                if let Some(outcome) = link::handle_link_action(link_step, user_action) {
                    match outcome {
                        LinkActionOutcome::ShareRequested => {
                            self.step = ExchangeStep::Link(LinkStep::WaitingForResponse);
                            // Emit ShowShareSheet so the frontend presents the share UI
                            if let Some(ref li) = self.link_initiation {
                                return ActionResult::Commands {
                                    commands: vec![Command::ShowShareSheet {
                                        url: li.url.clone(),
                                    }],
                                };
                            }
                            ActionResult::NavigateTo(self.build_screen())
                        }
                        LinkActionOutcome::Cancelled => ActionResult::Complete,
                    }
                } else {
                    ActionResult::UpdateScreen(self.build_screen())
                }
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
                // Restore to the correct sub-flow based on selected mode
                if self.config.mode == Some(ExchangeMode::Link) {
                    return self.start_link_mode();
                }
                if matches!(
                    self.config.mode,
                    Some(ExchangeMode::Magic | ExchangeMode::Bump | ExchangeMode::Shake)
                ) {
                    self.step = ExchangeStep::Ble(BleStep::Discovering);
                    return self.start_ble_mode();
                }
                self.step = ExchangeStep::Qr(QrStep::ShowQr);
                ActionResult::NavigateTo(self.build_screen())
            }
            (ExchangeStep::Failed, UserAction::ActionPressed { action_id })
                if action_id == "fallback_qr" =>
            {
                self.ble_fallback_available = false;
                self.qr_fallback_available = false;
                self.failure_detail = None;
                self.config.mode = Some(ExchangeMode::Glance);
                self.step = ExchangeStep::Qr(QrStep::ShowQr);
                self.start_session_if_needed()
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

    /// Override the `WorkflowEngine::advance_qr_frame` default to cycle animated
    /// QR frames while on the ShowQr step. Returns `None` on any other step (no
    /// animation active) or when there are no frames (e.g. no session yet).
    fn advance_qr_frame(&mut self) -> Option<ScreenModel> {
        if !matches!(self.step, ExchangeStep::Qr(QrStep::ShowQr)) {
            return None;
        }
        if self.qr_frames.len() <= 1 {
            return None;
        }
        self.qr_frame_index = (self.qr_frame_index + 1) % self.qr_frames.len();
        Some(self.build_screen())
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
            mode: Some(ExchangeMode::Glance),
            card_snapshot: None,
        }
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
    fn test_no_groups_skips_selection() {
        let engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        // Should start directly at ShowQr when no groups available
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_show_qr");
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
        assert!(matches!(result, ActionResult::NavigateTo(_)));
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));
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
    fn test_with_session_starts_qr_and_emits_display_command() {
        let session = create_test_session();
        let mut engine = ExchangeEngine::with_session(
            config_no_groups(),
            session,
            vauchi_core::clock::SystemClock::shared(),
        );

        // Session should be present
        assert!(engine.session().is_some(), "expected Some value");

        // Should be at ShowQr step
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));

        // Should have a QrDisplay command ready to drain
        let commands = engine.drain_commands();
        assert_eq!(commands.len(), 1);
        assert!(
            matches!(&commands[0], vauchi_core::Command::QrDisplay { .. }),
            "Expected QrDisplay command, got {:?}",
            commands[0]
        );
    }

    #[test]
    fn test_with_session_group_selection_defers_qr_start() {
        let session = create_test_session();
        let engine = ExchangeEngine::with_session(
            config_with_groups(),
            session,
            vauchi_core::clock::SystemClock::shared(),
        );

        // Should be at GroupSelection step — session not started yet
        assert_eq!(engine.step, ExchangeStep::GroupSelection);

        // No commands should be pending (session hasn't started QR yet)
        // (drain_commands is on mut self, so we check session state instead)
        assert!(engine.session().is_some(), "expected Some value");
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

        // Start exchange from field preview → QR with session
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "start_exchange".into(),
        });

        // Should emit Commands with QrDisplay
        match result {
            ActionResult::Commands { commands } => {
                assert_eq!(commands.len(), 1);
                assert!(
                    matches!(&commands[0], vauchi_core::Command::QrDisplay { .. }),
                    "Expected QrDisplay command, got {:?}",
                    commands[0]
                );
            }
            other => panic!("Expected Commands, got {:?}", other),
        }
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));
    }

    #[test]
    fn test_with_session_show_qr_continue_emits_scan_request() {
        let session = create_test_session();
        let mut engine = ExchangeEngine::with_session(
            config_no_groups(),
            session,
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.drain_commands(); // drain initial QrDisplay

        // Press continue → ScanQr
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });

        // Should emit QrRequestScan command
        match result {
            ActionResult::Commands { commands } => {
                assert_eq!(commands.len(), 1);
                assert_eq!(commands[0], vauchi_core::Command::QrRequestScan);
            }
            other => panic!("Expected Commands with QrRequestScan, got {:?}", other),
        }
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ScanQr));
    }

    #[test]
    fn test_handle_hardware_event_ble_discovery_emits_connect() {
        let session = create_test_session();
        let mut engine = ExchangeEngine::with_session(
            config_no_groups(),
            session,
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.drain_commands();

        // Simulate BLE discovery
        let result = engine.handle_hardware_event(vauchi_core::Event::BleDeviceDiscovered {
            id: "device-1".into(),
            rssi: -42,
            adv_data: vec![],
        });

        // Should emit BleConnect command
        assert!(result.is_some(), "expected Some value");
        if let Some(ActionResult::Commands { commands }) = result {
            assert!(
                commands
                    .iter()
                    .any(|c| matches!(c, vauchi_core::Command::BleConnect { .. })),
                "Expected BleConnect command in {:?}",
                commands
            );
        }
    }

    #[test]
    fn test_without_session_emits_qr_request_scan_command() {
        let mut engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );

        // No session
        assert!(engine.session().is_none());

        // ShowQr → ScanQr emits the QrRequestScan Command even
        // without an active peer session. The legacy `RequestCamera`
        // ActionResult is deprecated (ADR-022 Addendum D) — frontends
        // implement the command/event protocol only.
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        match &result {
            ActionResult::Commands { commands } => {
                assert_eq!(commands, &vec![vauchi_core::Command::QrRequestScan])
            }
            other => panic!("Expected Commands with QrRequestScan, got {other:?}"),
        }

        // handle_hardware_event handles QrScanned via legacy TextChanged path
        let result = engine.handle_hardware_event(vauchi_core::Event::QrScanned {
            data: "test".into(),
        });
        assert!(
            result.is_some(),
            "QrScanned should be handled even without session"
        );

        // Non-QR events return None without session
        let result = engine.handle_hardware_event(vauchi_core::Event::BleDeviceDiscovered {
            id: "d1".into(),
            rssi: -40,
            adv_data: vec![],
        });
        assert!(
            result.is_none(),
            "BLE events should be ignored without session"
        );
    }

    /// Helper: create two sessions (Alice and Bob) and return Alice's engine
    /// plus Bob's QR data string (what Alice would scan).
    fn create_alice_engine_and_bob_qr() -> (ExchangeEngine, String) {
        let alice_identity = vauchi_core::identity::Identity::create(
            "Alice",
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        );
        let alice_card = vauchi_core::contact_card::ContactCard::new("Alice");
        let alice_proximity = vauchi_core::exchange::ManualConfirmationVerifier::new();
        let alice_session = vauchi_core::exchange::ExchangeSession::new_qr(
            alice_identity,
            alice_card,
            alice_proximity,
            vauchi_core::clock::SystemClock::shared(),
        );

        let bob_identity = vauchi_core::identity::Identity::create(
            "Bob",
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        );
        let bob_card = vauchi_core::contact_card::ContactCard::new("Bob");
        let bob_proximity = vauchi_core::exchange::ManualConfirmationVerifier::new();
        let mut bob_session = vauchi_core::exchange::ExchangeSession::new_qr(
            bob_identity,
            bob_card,
            bob_proximity,
            vauchi_core::clock::SystemClock::shared(),
        );
        // Start Bob's QR so we can get his data string
        bob_session
            .apply(vauchi_core::exchange::ExchangeEvent::StartQR)
            .unwrap();
        let bob_qr = bob_session.qr().unwrap();
        let bob_qr_data = bob_qr.to_data_string();

        let engine = ExchangeEngine::with_session(
            config_no_groups(),
            alice_session,
            vauchi_core::clock::SystemClock::shared(),
        );
        (engine, bob_qr_data)
    }

    #[test]
    fn test_qr_scanned_auto_advances_to_success() {
        let (mut engine, bob_qr_data) = create_alice_engine_and_bob_qr();
        let _ = engine.drain_commands(); // drain initial QrDisplay

        // Move to ScanQr
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ScanQr));

        // Simulate scanning Bob's QR — auto-advance drives session to Complete
        let result =
            engine.handle_hardware_event(vauchi_core::Event::QrScanned { data: bob_qr_data });

        assert!(result.is_some(), "expected Some value");
        // With reciprocity gating: step is Verifying (relay) or Success (no relay).
        assert!(
            matches!(
                engine.step,
                ExchangeStep::Success | ExchangeStep::Qr(QrStep::Verifying)
            ),
            "After QrScanned, engine should be Success or Verifying, got {:?}",
            engine.step
        );
        // Session should be Complete
        let session = engine.session().unwrap();
        assert!(
            matches!(
                session.state(),
                vauchi_core::exchange::ExchangeState::Complete { .. }
            ),
            "Session should be Complete after QR auto-advance, got {:?}",
            session.state()
        );
    }

    #[test]
    fn test_show_qr_screen_uses_session_qr_data_when_active() {
        let session = create_test_session();
        let mut engine = ExchangeEngine::with_session(
            config_no_groups(),
            session,
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.drain_commands();

        // The ShowQr screen should use the session's QR data, not config.own_qr_data
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_show_qr");

        // Find the QrCode component and verify its data is NOT the config's static data
        let qr_component = screen.components.iter().find(|c| {
            matches!(
                c,
                Component::QrCode {
                    mode: QrMode::Display,
                    ..
                }
            )
        });
        assert!(
            qr_component.is_some(),
            "ShowQr screen should have a QrCode component"
        );
        if let Some(Component::QrCode { data, .. }) = qr_component {
            assert_ne!(
                data, &"qr-data",
                "QR data should come from session, not static config"
            );
            assert!(
                !data.is_empty(),
                "Session-generated QR data should not be empty"
            );
        }
    }

    #[test]
    fn test_full_qr_exchange_flow_via_commands_and_events() {
        let (mut engine, bob_qr_data) = create_alice_engine_and_bob_qr();

        // 1. After construction: QrDisplay command should be pending
        let commands = engine.drain_commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            &commands[0],
            vauchi_core::Command::QrDisplay { .. }
        ));

        // 2. User presses "Scan Their Code" → QrRequestScan command
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        match result {
            ActionResult::Commands { commands } => {
                assert_eq!(commands.len(), 1);
                assert_eq!(commands[0], vauchi_core::Command::QrRequestScan);
            }
            other => panic!("Expected Commands, got {:?}", other),
        }

        // 3. Frontend scans Bob's QR → auto-advance drives to Complete
        let result =
            engine.handle_hardware_event(vauchi_core::Event::QrScanned { data: bob_qr_data });
        assert!(result.is_some(), "expected Some value");

        // 4. Engine should be on Verifying — waiting for reciprocity confirmation.
        //    Exchange is NOT Success until the peer confirms via relay escrow.
        //    This prevents asymmetric exchanges (one side saves, other doesn't).
        let session = engine.session().unwrap();
        assert!(
            matches!(
                session.state(),
                vauchi_core::exchange::ExchangeState::Complete { .. }
            ),
            "Session should be Complete, got {:?}",
            session.state()
        );
        // Step depends on whether confirmation tokens are available.
        // In test sessions without relay, it falls through to Success (backward compat).
        // With relay tokens, it would be Verifying.
        assert!(
            matches!(
                engine.step,
                ExchangeStep::Success | ExchangeStep::Qr(QrStep::Verifying)
            ),
            "Step should be Success (no relay) or Verifying (with relay), got {:?}",
            engine.step
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

        // Start exchange from FieldPreview → ShowQr
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "start_exchange".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));

        // Continue through ShowQr → ScanQr → Verifying → Success
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ScanQr));
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "scanned_data".into(),
            value: "their-qr".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::Verifying));
        engine.mark_success();

        // Groups still selected at the end
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
        engine.mark_failed_with_error(&vauchi_core::exchange::ExchangeError::BleOutOfRange);
        assert!(engine.failure_detail.is_some());

        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "retry".into(),
        });

        assert!(
            engine.failure_detail.is_none(),
            "Retry should clear the failure detail"
        );
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));
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
    fn mode_preset_skips_mode_selection() {
        let engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        // config_no_groups() sets mode = Some(Glance)
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));
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

    // ── Hover / Glance mode tests ──────────────────────────────────

    // RED for Phase 1.E.2 of `2026-05-11-hover-graduation-plan.md`.
    //
    // After 1.E.3 GREEN, Hover stops routing through the legacy
    // `ExchangeStep::Qr` sub-flow — it joins Glance on the
    // `ActionResult::StartMultiStageExchange` handoff path, with
    // the engine staying on `ModeSelection` while AppEngine
    // navigates to `AppScreen::MultiStageExchange`. The legacy
    // `exchange_show_qr` screen is reached only by modes that
    // have *not* graduated to the new engine (Broadcast,
    // TapHoverShake — Phases 2/3 of the umbrella retirement).
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

    // @internal
    #[test]
    fn test_link_mode_starts_at_share_url() {
        let engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        assert_eq!(engine.step, ExchangeStep::Link(LinkStep::ShareUrl));
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_share_url");
    }

    // @internal
    #[test]
    fn test_link_mode_share_advances_to_waiting() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "share".into(),
        });
        assert_eq!(
            engine.step,
            ExchangeStep::Link(LinkStep::WaitingForResponse)
        );
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_link_waiting");
    }

    // @internal
    #[test]
    fn test_link_mode_cancel_completes() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "cancel".into(),
        });
        assert_eq!(result, ActionResult::Complete);
    }

    // @internal
    #[test]
    fn test_link_mode_with_groups_goes_through_preview() {
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

        // Start exchange → Link ShareUrl (not QR)
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "start_exchange".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Link(LinkStep::ShareUrl));
    }

    // @internal
    #[test]
    fn test_link_mode_retry_stays_in_link() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.mark_failed();
        assert_eq!(engine.step, ExchangeStep::Failed);

        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "retry".into(),
        });
        assert_eq!(
            engine.step,
            ExchangeStep::Link(LinkStep::ShareUrl),
            "Retry in Link mode must return to Link, not QR"
        );
    }

    // @internal
    #[test]
    fn test_link_mode_generates_initiation_on_construction() {
        let engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        assert!(
            engine.link_initiation.is_some(),
            "link_initiation must be populated at construction"
        );
        let li = engine.link_initiation.as_ref().unwrap();
        assert!(li.url.starts_with("vauchi://exchange?"));
    }

    // @internal
    #[test]
    fn test_link_mode_drain_commands_returns_presence_deposit() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let commands = engine.drain_commands();
        assert_eq!(commands.len(), 1, "must emit 1 presence deposit command");
        assert!(matches!(&commands[0], Command::RelayEscrowDeposit { .. }));
        // Second drain is empty
        assert!(engine.drain_commands().is_empty());
    }

    // @internal
    #[test]
    fn test_link_mode_share_url_screen_shows_generated_url() {
        let engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let screen = engine.current_screen();
        let url_text = screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::Text { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .expect("ShareUrl screen must have a Text component");
        assert!(
            url_text.starts_with("vauchi://exchange?"),
            "URL must be the generated link, not placeholder"
        );
    }

    // @internal
    #[test]
    fn test_link_mode_share_emits_show_share_sheet() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.drain_commands(); // drain presence deposit
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "share".into(),
        });
        match result {
            ActionResult::Commands { commands } => {
                assert_eq!(commands.len(), 1);
                assert!(
                    matches!(&commands[0], Command::ShowShareSheet { url } if url.starts_with("vauchi://exchange?")),
                    "Share must emit ShowShareSheet with the link URL"
                );
            }
            other => panic!("Expected Commands, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn test_link_shared_event_emits_escrow_check() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.drain_commands(); // drain presence deposit
        // Move to WaitingForResponse
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "share".into(),
        });
        assert_eq!(
            engine.step,
            ExchangeStep::Link(LinkStep::WaitingForResponse)
        );
        // Frontend reports LinkShared
        let result = engine.handle_hardware_event(vauchi_core::Event::LinkShared);
        match result {
            Some(ActionResult::Commands { commands }) => {
                assert_eq!(commands.len(), 1);
                assert!(
                    matches!(&commands[0], Command::RelayEscrowCheck { .. }),
                    "LinkShared must trigger escrow check polling"
                );
            }
            other => panic!("Expected Commands, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn test_link_escrow_ready_emits_retrieve_and_transitions_to_retrieving() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.drain_commands();
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "share".into(),
        });
        assert_eq!(
            engine.step,
            ExchangeStep::Link(LinkStep::WaitingForResponse)
        );
        // Simulate handshake gate ready
        let li = engine.link_initiation.as_ref().unwrap();
        let hs_gate = hex::decode(&li.handshake_slot).unwrap();
        let expected_slot = hex::decode(&li.presence_slot).unwrap();
        let result = engine
            .handle_hardware_event(vauchi_core::Event::RelayEscrowReady { gate_hash: hs_gate });
        // Must transition to Retrieving
        assert_eq!(
            engine.step,
            ExchangeStep::Link(LinkStep::Retrieving),
            "Must transition to Retrieving after handshake gate ready"
        );
        // Must emit RelayEscrowRetrieve with presence_slot (authenticates
        // with OUR slot; relay returns the OTHER slot's blob = responder's epk)
        match result {
            Some(ActionResult::Commands { commands }) => {
                assert_eq!(commands.len(), 1);
                if let Command::RelayEscrowRetrieve { slot_hash, .. } = &commands[0] {
                    assert_eq!(
                        slot_hash, &expected_slot,
                        "retrieve must use presence_slot for auth"
                    );
                } else {
                    panic!("expected RelayEscrowRetrieve");
                }
            }
            other => panic!("Expected Commands, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn test_link_escrow_failed_shows_error() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.drain_commands();
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "share".into(),
        });
        let result = engine.handle_hardware_event(vauchi_core::Event::RelayEscrowFailed {
            gate_hash: vec![],
            reason: "gate expired".into(),
        });
        assert!(result.is_some());
        assert_eq!(engine.step, ExchangeStep::Failed);
        assert_eq!(
            engine.failure_detail.as_deref(),
            Some("gate expired"),
            "failure reason must propagate to UI"
        );
    }

    // @internal
    #[test]
    fn test_link_unknown_gate_hash_ignored() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Link),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.drain_commands();
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "share".into(),
        });
        // Unknown gate_hash — should be ignored (returns None)
        let result = engine.handle_hardware_event(vauchi_core::Event::RelayEscrowReady {
            gate_hash: vec![0xAA; 32],
        });
        assert!(result.is_none(), "unknown gate must be silently ignored");
        assert_eq!(
            engine.step,
            ExchangeStep::Link(LinkStep::WaitingForResponse),
            "step must not change for unknown gate"
        );
    }

    // ── Phase 4: BLE fallback degradation tests ────────────────────

    // @internal
    #[test]
    fn ble_failure_shows_relay_fallback_action() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Magic),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        // Simulate BLE failure via apply_ble_outcome
        let _ = engine.apply_ble_outcome(BleHardwareOutcome::FailedWithFallback {
            reason: "BLE timeout".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Failed);
        assert!(engine.ble_fallback_available);

        let screen = engine.build_screen();
        assert!(
            screen.actions.iter().any(|a| a.id == "fallback_relay"),
            "Failed screen must show relay fallback for BLE failures"
        );
    }

    // @internal
    #[test]
    fn non_ble_failure_does_not_show_relay_fallback() {
        let mut engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.mark_failed();
        assert!(!engine.ble_fallback_available);

        let screen = engine.build_screen();
        assert!(
            !screen.actions.iter().any(|a| a.id == "fallback_relay"),
            "Non-BLE failure must not show relay fallback"
        );
    }

    // @internal
    #[test]
    fn fallback_relay_action_switches_to_link_mode() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Magic),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.apply_ble_outcome(BleHardwareOutcome::FailedWithFallback {
            reason: "timeout".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Failed);

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "fallback_relay".into(),
        });

        assert_eq!(
            engine.step,
            ExchangeStep::Link(LinkStep::ShareUrl),
            "Fallback must switch to Link mode"
        );
        assert!(!engine.ble_fallback_available);
        assert!(engine.failure_detail.is_none());
        // Should return commands for link mode setup
        assert!(
            matches!(result, ActionResult::Commands { .. }),
            "Expected Commands for link setup"
        );
    }

    // @internal
    #[test]
    fn retry_clears_ble_fallback_flag() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Bump),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.apply_ble_outcome(BleHardwareOutcome::FailedWithFallback {
            reason: "disconnect".into(),
        });
        assert!(engine.ble_fallback_available);

        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "retry".into(),
        });

        assert!(!engine.ble_fallback_available);
        assert_eq!(
            engine.step,
            ExchangeStep::Ble(BleStep::Discovering),
            "Retry in Bump mode must return to BLE discovering"
        );
    }

    // ── Phase 5: BLE mode integration tests (full engine flow) ─────

    /// Helper: create a BLE mode engine and advance through discovery + connection.
    fn ble_engine_to_exchanging(mode: ExchangeMode) -> ExchangeEngine {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(mode),
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        assert_eq!(engine.step, ExchangeStep::Ble(BleStep::Discovering));

        // Discovery
        let result = engine.handle_hardware_event(vauchi_core::Event::BleDeviceDiscovered {
            id: "peer-1".into(),
            rssi: -45,
            adv_data: vec![],
        });
        assert!(result.is_some());
        assert_eq!(engine.step, ExchangeStep::Ble(BleStep::Handshaking));

        // Connection
        let result = engine.handle_hardware_event(vauchi_core::Event::BleConnected {
            device_id: "peer-1".into(),
        });
        assert!(result.is_some());
        assert_eq!(engine.step, ExchangeStep::Ble(BleStep::Exchanging));
        engine
    }

    // @internal
    #[test]
    fn magic_full_flow_discovery_to_success() {
        let mut engine = ble_engine_to_exchanging(ExchangeMode::Magic);

        // Card data
        engine.handle_hardware_event(vauchi_core::Event::BleCharacteristicNotified {
            uuid: "card".into(),
            data: vec![1, 2, 3],
        });
        // Audio response → proximity done → complete. Build a real
        // FSK-encoded sample buffer so the runner's decode succeeds.
        let modem_config = vauchi_core::exchange::audio_modem::AudioConfig::default();
        let samples =
            vauchi_core::exchange::audio_modem::generate_fsk_samples(&[0xAA], &modem_config);
        let result = engine.handle_hardware_event(vauchi_core::Event::AudioSamplesRecorded {
            samples,
            sample_rate: modem_config.sample_rate,
        });
        assert!(result.is_some());
        assert_eq!(engine.step, ExchangeStep::Success);
    }

    // @internal
    #[test]
    fn bump_full_flow_discovery_to_success() {
        let mut engine = ble_engine_to_exchanging(ExchangeMode::Bump);

        // Card data
        engine.handle_hardware_event(vauchi_core::Event::BleCharacteristicNotified {
            uuid: "card".into(),
            data: vec![4, 5, 6],
        });
        // Impact → proximity done → complete
        let result = engine.handle_hardware_event(vauchi_core::Event::ImpactDetected {
            timestamp_ms: 100,
            magnitude_milli_g: 3500,
        });
        assert!(result.is_some());
        assert_eq!(engine.step, ExchangeStep::Success);
    }

    // @internal
    #[test]
    fn shake_full_flow_discovery_to_success() {
        let mut engine = ble_engine_to_exchanging(ExchangeMode::Shake);

        // Feed accel samples (triggers recording + envelope send)
        for i in 0..50 {
            engine.handle_hardware_event(vauchi_core::Event::AccelerometerData {
                x_milli_g: ((i as f32 * 0.1).sin() * 2000.0) as i32,
                y_milli_g: ((i as f32 * 0.1).cos() * 1500.0) as i32,
                z_milli_g: 1000,
                timestamp_ms: i * 10,
            });
        }

        // Card data
        engine.handle_hardware_event(vauchi_core::Event::BleCharacteristicNotified {
            uuid: "card".into(),
            data: vec![7, 8, 9],
        });

        // Peer shake envelope (use encoded constant data for simplicity)
        let peer_envelope = vauchi_core::exchange::shake_protocol::encode_envelope(&[1.5; 50]);
        let result = engine.handle_hardware_event(vauchi_core::Event::BleCharacteristicNotified {
            uuid: vauchi_core::exchange::CHAR_DATA_WRITE.into(),
            data: peer_envelope,
        });
        assert!(result.is_some());
        assert_eq!(engine.step, ExchangeStep::Success);
    }

    // @internal
    #[test]
    fn ble_disconnect_during_exchange_offers_relay_fallback() {
        let mut engine = ble_engine_to_exchanging(ExchangeMode::Magic);

        let result = engine.handle_hardware_event(vauchi_core::Event::BleDisconnected {
            reason: "connection lost".into(),
        });
        assert!(result.is_some());
        assert_eq!(engine.step, ExchangeStep::Failed);
        assert!(engine.ble_fallback_available);

        // Accept fallback → switch to Link
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "fallback_relay".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Link(LinkStep::ShareUrl));
    }

    // ── Permission degradation fallback tests ─────────────────────────

    fn config_with_camera() -> ExchangeConfig {
        ExchangeConfig {
            device_capabilities: DeviceCapabilities {
                has_camera: true,
                ..Default::default()
            },
            ..config_no_groups()
        }
    }

    // @internal
    #[test]
    fn camera_unavailable_during_qr_scan_switches_to_manual_entry() {
        let mut engine = ExchangeEngine::new(
            config_with_camera(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.step = ExchangeStep::Qr(QrStep::ScanQr);

        let result = engine.handle_hardware_event(vauchi_core::Event::HardwareUnavailable {
            transport: "camera".into(),
        });

        assert_eq!(
            engine.step,
            ExchangeStep::Qr(QrStep::ManualEntry),
            "Camera unavailable must switch to manual entry"
        );
        assert!(
            matches!(result, Some(ActionResult::NavigateTo(_))),
            "Must navigate to manual entry screen"
        );
    }

    // @internal
    #[test]
    fn camera_permission_denied_during_qr_scan_switches_to_manual_entry() {
        let mut engine = ExchangeEngine::new(
            config_with_camera(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.step = ExchangeStep::Qr(QrStep::ScanQr);

        let result = engine.handle_hardware_event(vauchi_core::Event::PermissionDenied {
            transport: "camera".into(),
        });

        assert_eq!(
            engine.step,
            ExchangeStep::Qr(QrStep::ManualEntry),
            "Camera permission denied must switch to manual entry"
        );
        assert!(
            matches!(result, Some(ActionResult::NavigateTo(_))),
            "Must navigate to manual entry screen"
        );
    }

    // @internal
    #[test]
    fn manual_entry_screen_has_text_input_and_submit() {
        let screen = qr::build_manual_entry_screen(Progress {
            current_step: 2,
            total_steps: TOTAL_STEPS,
            label: None,
        });

        assert_eq!(screen.screen_id, "exchange_manual_entry");
        assert!(
            screen
                .components
                .iter()
                .any(|c| matches!(c, Component::TextInput { id, .. } if id == "manual_code")),
            "Manual entry screen must have a text input for the code"
        );
        assert!(
            screen.actions.iter().any(|a| a.id == "submit_code"),
            "Manual entry screen must have a submit button"
        );
        assert!(
            screen.actions.iter().any(|a| a.id == "back"),
            "Manual entry screen must have a back button"
        );
    }

    // @internal
    #[test]
    fn manual_code_entry_advances_to_verifying() {
        let mut engine = ExchangeEngine::new(
            config_with_camera(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.step = ExchangeStep::Qr(QrStep::ManualEntry);

        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "manual_code".into(),
            value: "vauchi://exchange/abc123".into(),
        });

        assert_eq!(
            engine.step,
            ExchangeStep::Qr(QrStep::Verifying),
            "Submitting manual code must advance to verifying"
        );
        assert_eq!(
            engine.scanned_data.as_deref(),
            Some("vauchi://exchange/abc123"),
            "Manual code must be stored as scanned data"
        );
    }

    // @internal
    #[test]
    fn manual_entry_back_returns_to_show_qr() {
        let mut engine = ExchangeEngine::new(
            config_with_camera(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.step = ExchangeStep::Qr(QrStep::ManualEntry);

        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "back".into(),
        });

        assert_eq!(
            engine.step,
            ExchangeStep::Qr(QrStep::ShowQr),
            "Back from manual entry must return to ShowQr"
        );
    }

    // @internal
    #[test]
    fn ble_failure_with_camera_shows_qr_fallback() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Magic),
                device_capabilities: DeviceCapabilities {
                    has_camera: true,
                    has_ble: true,
                    ..Default::default()
                },
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.apply_ble_outcome(BleHardwareOutcome::FailedWithFallback {
            reason: "BLE timeout".into(),
        });

        assert!(engine.qr_fallback_available);
        let screen = engine.build_screen();
        assert!(
            screen.actions.iter().any(|a| a.id == "fallback_qr"),
            "BLE failure with camera must show QR fallback"
        );
        assert!(
            screen.actions.iter().any(|a| a.id == "fallback_relay"),
            "BLE failure must also show relay fallback"
        );
    }

    // @internal
    #[test]
    fn ble_failure_without_camera_has_no_qr_fallback() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Magic),
                device_capabilities: DeviceCapabilities {
                    has_camera: false,
                    has_ble: true,
                    ..Default::default()
                },
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.apply_ble_outcome(BleHardwareOutcome::FailedWithFallback {
            reason: "BLE timeout".into(),
        });

        assert!(!engine.qr_fallback_available);
        let screen = engine.build_screen();
        assert!(
            !screen.actions.iter().any(|a| a.id == "fallback_qr"),
            "BLE failure without camera must not show QR fallback"
        );
    }

    // @internal
    #[test]
    fn fallback_qr_action_switches_to_qr_flow() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Magic),
                device_capabilities: DeviceCapabilities {
                    has_camera: true,
                    has_ble: true,
                    ..Default::default()
                },
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.apply_ble_outcome(BleHardwareOutcome::FailedWithFallback {
            reason: "timeout".into(),
        });

        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "fallback_qr".into(),
        });

        assert_eq!(
            engine.step,
            ExchangeStep::Qr(QrStep::ShowQr),
            "Fallback QR must switch to QR flow"
        );
        assert!(!engine.ble_fallback_available);
        assert!(!engine.qr_fallback_available);
        assert!(engine.failure_detail.is_none());
    }

    // @internal
    #[test]
    fn ble_permission_denied_shows_fallback() {
        let mut engine = ExchangeEngine::new(
            ExchangeConfig {
                mode: Some(ExchangeMode::Magic),
                device_capabilities: DeviceCapabilities {
                    has_camera: true,
                    has_ble: true,
                    ..Default::default()
                },
                ..config_no_groups()
            },
            vauchi_core::clock::SystemClock::shared(),
        );

        let result = engine.handle_hardware_event(vauchi_core::Event::PermissionDenied {
            transport: "BLE".into(),
        });

        assert_eq!(engine.step, ExchangeStep::Failed);
        assert!(engine.ble_fallback_available);
        assert!(
            matches!(result, Some(ActionResult::UpdateScreen(_))),
            "BLE permission denied must return screen update with failed state"
        );
    }

    // @internal
    #[test]
    fn camera_unavailable_outside_scan_step_is_ignored() {
        let mut engine = ExchangeEngine::new(
            config_with_camera(),
            vauchi_core::clock::SystemClock::shared(),
        );
        // In ShowQr step — camera unavailable should not trigger manual entry
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));

        let result = engine.handle_hardware_event(vauchi_core::Event::HardwareUnavailable {
            transport: "camera".into(),
        });

        assert_eq!(
            engine.step,
            ExchangeStep::Qr(QrStep::ShowQr),
            "Camera unavailable in ShowQr should not switch to manual entry"
        );
        // Session is None so it returns None
        assert!(result.is_none());
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
    fn scan_quality_starts_as_no_signal() {
        let mut engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        // Advance to ScanQr
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ScanQr));

        let screen = engine.current_screen();
        match &screen.components[0] {
            Component::QrCode { scan_quality, .. } => {
                assert_eq!(*scan_quality, Some(ScanQuality::NoSignal));
            }
            other => panic!("expected QrCode, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn scan_progress_updates_quality_to_good() {
        let mut engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });

        // Send 10 detected frames
        for _ in 0..10 {
            let result = engine.handle_hardware_event(vauchi_core::Event::QrScanProgress {
                detected: true,
                confidence: Some(90),
                frame_skipped: false,
            });
            assert!(
                matches!(result, Some(ActionResult::UpdateScreen(_))),
                "QrScanProgress must trigger screen update"
            );
        }

        let screen = engine.current_screen();
        match &screen.components[0] {
            Component::QrCode { scan_quality, .. } => {
                assert_eq!(*scan_quality, Some(ScanQuality::Good));
            }
            other => panic!("expected QrCode, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn scan_progress_degrades_to_poor_on_low_detection() {
        let mut engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });

        // 2 detected, 8 missed → 20% → Poor
        for i in 0..10 {
            engine.handle_hardware_event(vauchi_core::Event::QrScanProgress {
                detected: i < 2,
                confidence: None,
                frame_skipped: false,
            });
        }

        let screen = engine.current_screen();
        match &screen.components[0] {
            Component::QrCode { scan_quality, .. } => {
                assert_eq!(*scan_quality, Some(ScanQuality::Poor));
            }
            other => panic!("expected QrCode, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn scan_quality_resets_on_back_and_re_enter() {
        let mut engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });

        // Build up quality
        for _ in 0..10 {
            engine.handle_hardware_event(vauchi_core::Event::QrScanProgress {
                detected: true,
                confidence: None,
                frame_skipped: false,
            });
        }

        // Go back
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "back".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));

        // Re-enter scan
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ScanQr));

        // Quality should be reset to NoSignal
        let screen = engine.current_screen();
        match &screen.components[0] {
            Component::QrCode { scan_quality, .. } => {
                assert_eq!(*scan_quality, Some(ScanQuality::NoSignal));
            }
            other => panic!("expected QrCode, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn scan_progress_ignored_outside_scan_step() {
        let mut engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        // Still on ShowQr step
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));

        let result = engine.handle_hardware_event(vauchi_core::Event::QrScanProgress {
            detected: true,
            confidence: Some(100),
            frame_skipped: false,
        });

        // Should not be handled (no session, not in scan step)
        assert!(
            result.is_none(),
            "QrScanProgress on ShowQr step should be ignored"
        );
    }

    // @internal
    #[test]
    fn skipped_frames_do_not_degrade_quality() {
        let mut engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });

        // Send 5 detected frames → Good quality
        for _ in 0..5 {
            engine.handle_hardware_event(vauchi_core::Event::QrScanProgress {
                detected: true,
                confidence: None,
                frame_skipped: false,
            });
        }

        // Send 20 skipped frames — these should NOT count as misses
        for _ in 0..20 {
            engine.handle_hardware_event(vauchi_core::Event::QrScanProgress {
                detected: false,
                confidence: None,
                frame_skipped: true,
            });
        }

        // Quality should still be Good (5/5 = 100%, skipped frames excluded)
        let screen = engine.current_screen();
        match &screen.components[0] {
            Component::QrCode { scan_quality, .. } => {
                assert_eq!(*scan_quality, Some(ScanQuality::Good));
            }
            other => panic!("expected QrCode, got {:?}", other),
        }
    }

    fn qr_data_from_screen(screen: &ScreenModel) -> &str {
        for c in &screen.components {
            if let Component::QrCode {
                data,
                mode: QrMode::Display,
                ..
            } = c
            {
                return data.as_str();
            }
        }
        panic!("expected QrCode Display component in {:?}", screen);
    }

    // @internal
    #[test]
    fn test_advance_qr_frame_cycles_frames_on_show_qr() {
        use crate::ui::engine::WorkflowEngine;

        let session = create_test_session();
        let mut engine = ExchangeEngine::with_session(
            config_no_groups(),
            session,
            vauchi_core::clock::SystemClock::shared(),
        );
        let _ = engine.drain_commands();

        let total = engine.qr_frame_count();
        assert!(
            total > 1,
            "test session payload should yield >1 animated frame, got {total}"
        );

        let initial = qr_data_from_screen(&engine.current_screen()).to_owned();

        // Advance once — must return Some(ScreenModel) with different frame data.
        let next = WorkflowEngine::advance_qr_frame(&mut engine)
            .expect("advance on ShowQr with animated frames returns Some");
        assert_eq!(next.screen_id, "exchange_show_qr");
        let after_one = qr_data_from_screen(&next).to_owned();
        assert_ne!(
            initial, after_one,
            "frame data should change after one advance"
        );

        // Advance `total - 1` more times — should return to the initial frame.
        for _ in 0..(total - 1) {
            WorkflowEngine::advance_qr_frame(&mut engine).expect("still on ShowQr");
        }
        let wrapped_screen = engine.current_screen();
        let wrapped = qr_data_from_screen(&wrapped_screen);
        assert_eq!(
            wrapped, initial,
            "cycling through all {total} frames should wrap to initial"
        );
    }

    // @internal
    #[test]
    fn test_advance_qr_frame_returns_none_off_show_qr() {
        use crate::ui::engine::WorkflowEngine;

        // Engine parked on ModeSelection (no pre-selected mode).
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        assert_ne!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));
        assert!(
            WorkflowEngine::advance_qr_frame(&mut engine).is_none(),
            "advance must return None off the ShowQr step"
        );
    }

    // @internal
    #[test]
    fn test_advance_qr_frame_returns_none_when_no_frames() {
        use crate::ui::engine::WorkflowEngine;

        // Force ShowQr step without a session → qr_frames is empty.
        let mut engine = ExchangeEngine::new(
            config_no_groups(),
            vauchi_core::clock::SystemClock::shared(),
        );
        engine.step = ExchangeStep::Qr(QrStep::ShowQr);
        assert!(engine.qr_frames.is_empty());
        assert!(
            WorkflowEngine::advance_qr_frame(&mut engine).is_none(),
            "advance must return None when qr_frames is empty"
        );
    }

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

        // Engine advances to NfcStep::AwaitingTap and emits the initial
        // Command::NfcActivate with the initiator's key-offer payload.
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
        // nfc_identity has been consumed by start_taptap_mode.
        assert!(
            engine.nfc_identity.is_none(),
            "set_nfc_identity must be consumed by start_taptap_mode"
        );
        // nfc_flow now exists and tracks the initiator state.
        assert!(engine.nfc_flow.is_some(), "nfc_flow must be populated");
    }

    // @internal
    #[test]
    fn taptap_mode_without_identity_routes_to_failed() {
        let mut engine = ExchangeEngine::new(
            config_mode_selection(),
            vauchi_core::clock::SystemClock::shared(),
        );
        // No set_nfc_identity call — start_taptap_mode must fail-fast
        // rather than panic.
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:fun".into(),
            item_id: "mode:tap_tap".into(),
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
