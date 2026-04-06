// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange engine — QR-based contact exchange workflow.
//!
//! ADR-031: ExchangeEngine holds an optional `ExchangeSession` to connect
//! the UI workflow with the cryptographic protocol state machine. When a
//! session is provided, transitions emit `ExchangeCommand`s that frontends
//! dispatch to platform hardware (camera, BLE, NFC, audio).

use crate::ui::exchange_ble::{
    self, BleActionOutcome, BleExchangeFlow, BleHardwareOutcome, BleStep,
};
use crate::ui::exchange_field_preview::{self, FieldPreviewConfig, FieldPreviewResult};
use crate::ui::exchange_link::{self, LinkActionOutcome, LinkHardwareOutcome, LinkStep};
use crate::ui::exchange_mode_selection::{ModeSelectionEngine, ModeSelectionResult};
use crate::ui::exchange_qr::{self, QrActionOutcome, QrStep};
use crate::ui::*;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::command::ExchangeCommand;
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
/// transitions to `ExchangeSession` and emits `ExchangeCommand`s via
/// `ActionResult::ExchangeCommands`. When `session` is `None`, the engine
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
    /// Mode selection sub-engine (created on demand).
    mode_selection: Option<ModeSelectionEngine>,
    /// Field preview config (built when entering FieldPreview step).
    field_preview: Option<FieldPreviewConfig>,
    /// Link mode initiation data (URL, nonce, secret key, handshake slot).
    /// Populated when entering Link(ShareUrl) via `initiator_generate()`.
    link_initiation: Option<LinkInitiation>,
    /// Pending Link mode commands (presence deposit, relay calls).
    /// Drained via `drain_commands()` same as session commands.
    pending_link_commands: Vec<ExchangeCommand>,
    /// Escrow keys derived after DH with responder (Link mode only).
    /// Populated on `LinkOpened` event, used for card retrieval + decryption.
    escrow_keys: Option<EscrowKeys>,
    /// Decrypted card bytes from Link mode exchange (set on ExchangeComplete).
    /// Callers check `link_received_card_bytes()` after Success to save the contact.
    link_received_card: Option<Vec<u8>>,
    /// BLE exchange flow state machine (Magic/Bump/Shake modes).
    ble_flow: Option<BleExchangeFlow>,
    /// Reciprocity confirmation cascade driver (created on exchange completion).
    reciprocity_confirmer: Option<ReciprocityConfirmer>,
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
    /// Link exchange sub-flow (async relay-mediated).
    Link(LinkStep),
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
            Self::Link(link) => link.step_number(4),
            Self::Success => 4 + QrStep::STEP_COUNT,
            Self::Failed => 5 + QrStep::STEP_COUNT,
        }
    }
}

// mode + group + preview + sub-flow + success/failed
// All sub-flows must have the same step count for consistent progress.
const _: () = assert!(QrStep::STEP_COUNT == LinkStep::STEP_COUNT);
const _: () = assert!(QrStep::STEP_COUNT == BleStep::STEP_COUNT);
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

    pub fn new(config: ExchangeConfig) -> Self {
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
            mode_selection,
            field_preview: None,
            link_initiation,
            pending_link_commands,
            escrow_keys: None,
            link_received_card: None,
            ble_flow,
            reciprocity_confirmer: None,
        }
    }

    /// Creates a new ExchangeEngine with a protocol session (ADR-031).
    ///
    /// When a session is provided, the engine emits `ExchangeCommand`s
    /// at each step transition, connecting the UI workflow with the
    /// cryptographic protocol state machine.
    ///
    /// If no group selection is needed, the session is started immediately
    /// (StartQR applied). Use `drain_commands()` to get the initial
    /// `QrDisplay` command after construction.
    pub fn with_session(config: ExchangeConfig, mut session: ExchangeSession) -> Self {
        let step = Self::initial_step(&config);
        let mode_selection = if step == ExchangeStep::ModeSelection {
            Some(ModeSelectionEngine::new(config.device_capabilities.clone()))
        } else {
            None
        };

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
                    mode_selection,
                    field_preview: None,
                    link_initiation: None,
                    pending_link_commands: Vec::new(),
                    escrow_keys: None,
                    link_received_card: None,
                    ble_flow: None,
                    reciprocity_confirmer: None,
                };
            }
            session.emit_initial_commands();
        }

        Self {
            step,
            config,
            scanned_data: None,
            selected_groups: Vec::new(),
            session: Some(session),
            failure_detail: None,
            ble_fallback_available: false,
            mode_selection,
            field_preview: None,
            link_initiation: None,
            pending_link_commands: Vec::new(),
            escrow_keys: None,
            link_received_card: None,
            ble_flow: None,
            reciprocity_confirmer: None,
        }
    }

    /// Drains any pending commands from the session or Link mode (ADR-031).
    ///
    /// Call this after construction with `with_session()` to get the
    /// initial `QrDisplay` command, or after `new()` with Link mode to get
    /// the initial presence deposit command.
    pub fn drain_commands(&mut self) -> Vec<ExchangeCommand> {
        if !self.pending_link_commands.is_empty() {
            return std::mem::take(&mut self.pending_link_commands);
        }
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
    /// emits initial commands, and returns `ExchangeCommands`.
    /// Otherwise, returns `NavigateTo` for legacy UI-only behavior.
    fn start_session_if_needed(&mut self) -> ActionResult {
        if let Some(ref mut session) = self.session {
            match session.apply(ExchangeEvent::StartQR) {
                Ok(()) => {
                    session.emit_initial_commands();
                    let commands = session.drain_commands();
                    if !commands.is_empty() {
                        return ActionResult::ExchangeCommands { commands };
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
    /// Returns `ExchangeCommands` with the presence deposit for the handshake
    /// gate, or `NavigateTo` if no commands are needed (shouldn't happen).
    fn start_link_mode(&mut self) -> ActionResult {
        self.step = ExchangeStep::Link(LinkStep::ShareUrl);
        let (initiation, commands) = link_mode::initiator_generate();
        self.link_initiation = Some(initiation);
        if commands.is_empty() {
            ActionResult::NavigateTo(self.build_screen())
        } else {
            ActionResult::ExchangeCommands { commands }
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
        ActionResult::ExchangeCommands {
            commands: vec![
                ExchangeCommand::BleStartAdvertising {
                    service_uuid: service_uuid.clone(),
                    payload: vec![],
                },
                ExchangeCommand::BleStartScanning { service_uuid },
            ],
        }
    }

    /// Handle BLE mode hardware events via BleExchangeFlow.
    fn handle_ble_hardware_event(
        &mut self,
        event: vauchi_core::exchange::ExchangeHardwareEvent,
    ) -> Option<ActionResult> {
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
                    ActionResult::ExchangeCommands { commands }
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
                    ActionResult::ExchangeCommands { commands }
                }
            }
            BleHardwareOutcome::FailedWithFallback { reason } => {
                self.failure_detail = Some(reason);
                self.ble_fallback_available = true;
                self.step = ExchangeStep::Failed;
                ActionResult::UpdateScreen(self.build_screen())
            }
            BleHardwareOutcome::Ignored => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    /// Handle Link mode hardware events (ADR-031).
    ///
    /// Routes to handshake-phase or escrow-phase handler depending on
    /// whether ECDH has completed (escrow_keys present).
    fn handle_link_hardware_event(
        &mut self,
        event: vauchi_core::exchange::ExchangeHardwareEvent,
    ) -> Option<ActionResult> {
        // Special case: LinkOpened triggers DH + card encryption
        if let vauchi_core::exchange::ExchangeHardwareEvent::LinkOpened {
            ref peer_public_key,
        } = event
        {
            return self.handle_link_opened(peer_public_key);
        }

        // Escrow phase: keys are known, handle card exchange events
        if let Some(ref keys) = self.escrow_keys
            && let Some(outcome) = exchange_link::handle_escrow_hw_event(keys, &event)
        {
            return Some(self.apply_link_outcome(outcome));
        }

        // Handshake phase: waiting for responder's epk
        let li = self.link_initiation.as_ref()?;
        let outcome = exchange_link::handle_link_hw_event(li, &event)?;
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
                exchange_link::handle_link_opened(li, peer_public_key, &card_bytes)
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
                ActionResult::ExchangeCommands { commands }
            }
            LinkHardwareOutcome::RetrieveFromHandshake { commands } => {
                self.step = ExchangeStep::Link(LinkStep::Retrieving);
                ActionResult::ExchangeCommands { commands }
            }
            LinkHardwareOutcome::DhCompleteCardDeposited {
                commands,
                escrow_keys,
            } => {
                self.escrow_keys = Some(escrow_keys);
                ActionResult::ExchangeCommands { commands }
            }
            LinkHardwareOutcome::RetrieveFromEscrow { commands } => {
                ActionResult::ExchangeCommands { commands }
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
                    exchange_field_preview::build_field_preview_screen(fp, self.progress())
                } else {
                    ScreenModel::default()
                }
            }
            ExchangeStep::Qr(QrStep::ShowQr) => exchange_qr::build_show_qr_screen(
                self.session.as_ref(),
                &self.config.own_name,
                &self.config.own_qr_data,
                self.progress(),
            ),
            ExchangeStep::Qr(QrStep::ScanQr) => exchange_qr::build_scan_qr_screen(self.progress()),
            ExchangeStep::Qr(QrStep::Verifying) => {
                exchange_qr::build_verifying_screen(self.progress())
            }
            ExchangeStep::Ble(BleStep::Discovering) => {
                let mode = self.config.mode.unwrap_or(ExchangeMode::Magic);
                exchange_ble::build_discovering_screen(mode, self.progress())
            }
            ExchangeStep::Ble(BleStep::Handshaking | BleStep::Exchanging) => {
                let mode = self.config.mode.unwrap_or(ExchangeMode::Magic);
                exchange_ble::build_exchanging_screen(mode, self.progress())
            }
            ExchangeStep::Ble(BleStep::Verifying) => {
                let mode = self.config.mode.unwrap_or(ExchangeMode::Magic);
                exchange_ble::build_verifying_screen(mode, self.progress())
            }
            ExchangeStep::Ble(BleStep::Complete) => {
                // Handled by transition to ExchangeStep::Success
                ScreenModel::default()
            }
            ExchangeStep::Link(LinkStep::ShareUrl) => {
                let url = self
                    .link_initiation
                    .as_ref()
                    .map(|li| li.url.as_str())
                    .unwrap_or("generating...");
                exchange_link::build_share_url_screen(url, self.progress())
            }
            ExchangeStep::Link(LinkStep::WaitingForResponse) => {
                exchange_link::build_waiting_screen(self.progress())
            }
            ExchangeStep::Link(LinkStep::Retrieving) => {
                exchange_link::build_retrieving_screen(self.progress())
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
                }],
                actions: vec![ScreenAction {
                    id: "done".into(),
                    label: "Done".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
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
                }];
                // BLE failures offer relay fallback (Phase 4 degradation)
                if self.ble_fallback_available {
                    actions.push(ScreenAction {
                        id: "fallback_relay".into(),
                        label: "Switch to encrypted relay".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                    });
                }
                actions.push(ScreenAction {
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
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
        }],
        actions: vec![
            ScreenAction {
                id: "continue".into(),
                label: "Continue".into(),
                style: ActionStyle::Primary,
                enabled: true,
            },
            ScreenAction {
                id: "skip".into(),
                label: "Skip".into(),
                style: ActionStyle::Secondary,
                enabled: true,
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

    fn handle_hardware_event(
        &mut self,
        event: vauchi_core::exchange::ExchangeHardwareEvent,
    ) -> Option<ActionResult> {
        // BLE mode events — routed through BleExchangeFlow
        if matches!(self.step, ExchangeStep::Ble(_)) {
            return self.handle_ble_hardware_event(event);
        }

        // Link mode events — handled without ExchangeSession
        if matches!(self.step, ExchangeStep::Link(_)) {
            return self.handle_link_hardware_event(event);
        }

        // No session — handle QR scan via legacy TextChanged path
        let session = match self.session.as_mut() {
            Some(s) => s,
            None => {
                if let vauchi_core::exchange::ExchangeHardwareEvent::QrScanned { data } = event {
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

        // Route escrow events to reciprocity confirmer if active
        if let Some(ref mut confirmer) = self.reciprocity_confirmer {
            if let Some(ref evt) = event_for_confirmer {
                let cmds = confirmer.handle_event(evt);
                commands.extend(cmds);
            }
            if confirmer.is_done() {
                self.reciprocity_confirmer = None;
            }
        }

        // Sync engine step from session state
        match session.state() {
            vauchi_core::exchange::ExchangeState::Complete { .. } => {
                self.step = ExchangeStep::Success;
                // Create reciprocity confirmer from session tokens
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
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        true,
                    );
                    commands.extend(confirmer.start());
                    self.reciprocity_confirmer = Some(confirmer);
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
            Some(ActionResult::ExchangeCommands { commands })
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
                if let Some(outcome) =
                    exchange_field_preview::handle_field_preview_action(user_action)
                {
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
            // QR sub-flow actions — delegated to exchange_qr module
            (ExchangeStep::Qr(qr_step), ref user_action) => {
                if let Some(outcome) =
                    exchange_qr::handle_qr_action(qr_step, user_action, self.session.is_some())
                {
                    match outcome {
                        QrActionOutcome::AdvanceToScan { session_active } => {
                            self.step = ExchangeStep::Qr(QrStep::ScanQr);
                            if session_active {
                                ActionResult::ExchangeCommands {
                                    commands: vec![ExchangeCommand::QrRequestScan],
                                }
                            } else {
                                ActionResult::RequestCamera
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
                    }
                } else {
                    ActionResult::UpdateScreen(self.build_screen())
                }
            }
            // BLE sub-flow actions
            (ExchangeStep::Ble(ble_step), ref user_action) => {
                if let Some(outcome) = exchange_ble::handle_ble_action(ble_step, user_action) {
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
                if let Some(outcome) = exchange_link::handle_link_action(link_step, user_action) {
                    match outcome {
                        LinkActionOutcome::ShareRequested => {
                            self.step = ExchangeStep::Link(LinkStep::WaitingForResponse);
                            // Emit ShowShareSheet so the frontend presents the share UI
                            if let Some(ref li) = self.link_initiation {
                                return ActionResult::ExchangeCommands {
                                    commands: vec![ExchangeCommand::ShowShareSheet {
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
                if action_id == "fallback_relay" =>
            {
                self.ble_fallback_available = false;
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
        let engine = ExchangeEngine::new(config_no_groups());
        // Should start directly at ShowQr when no groups available
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_show_qr");
    }

    #[test]
    fn test_with_groups_starts_at_selection() {
        let engine = ExchangeEngine::new(config_with_groups());
        // Should start at GroupSelection when groups exist
        assert_eq!(engine.step, ExchangeStep::GroupSelection);
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_group_selection");
    }

    #[test]
    fn test_group_selection_toggle_and_continue() {
        let mut engine = ExchangeEngine::new(config_with_groups());

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
        let mut engine = ExchangeEngine::new(config_with_groups());

        // Skip without selecting any groups
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "skip".into(),
        });
        assert!(matches!(result, ActionResult::NavigateTo(_)));
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));
        assert!(engine.selected_groups().is_empty());
    }

    // ── ADR-031: ExchangeSession integration tests ──────────────────

    fn create_test_session() -> vauchi_core::exchange::ExchangeSession {
        let identity = vauchi_core::identity::Identity::create("TestUser");
        let card = vauchi_core::contact_card::ContactCard::new("TestUser");
        let proximity = vauchi_core::exchange::ManualConfirmationVerifier::new();
        vauchi_core::exchange::ExchangeSession::new_qr(identity, card, proximity)
    }

    #[test]
    fn test_with_session_starts_qr_and_emits_display_command() {
        let session = create_test_session();
        let mut engine = ExchangeEngine::with_session(config_no_groups(), session);

        // Session should be present
        assert!(engine.session().is_some(), "expected Some value");

        // Should be at ShowQr step
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));

        // Should have a QrDisplay command ready to drain
        let commands = engine.drain_commands();
        assert_eq!(commands.len(), 1);
        assert!(
            matches!(
                &commands[0],
                vauchi_core::exchange::command::ExchangeCommand::QrDisplay { .. }
            ),
            "Expected QrDisplay command, got {:?}",
            commands[0]
        );
    }

    #[test]
    fn test_with_session_group_selection_defers_qr_start() {
        let session = create_test_session();
        let engine = ExchangeEngine::with_session(config_with_groups(), session);

        // Should be at GroupSelection step — session not started yet
        assert_eq!(engine.step, ExchangeStep::GroupSelection);

        // No commands should be pending (session hasn't started QR yet)
        // (drain_commands is on mut self, so we check session state instead)
        assert!(engine.session().is_some(), "expected Some value");
    }

    #[test]
    fn test_with_session_group_continue_shows_field_preview() {
        let session = create_test_session();
        let mut engine = ExchangeEngine::with_session(config_with_groups(), session);

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

        // Should emit ExchangeCommands with QrDisplay
        match result {
            ActionResult::ExchangeCommands { commands } => {
                assert_eq!(commands.len(), 1);
                assert!(
                    matches!(
                        &commands[0],
                        vauchi_core::exchange::command::ExchangeCommand::QrDisplay { .. }
                    ),
                    "Expected QrDisplay command, got {:?}",
                    commands[0]
                );
            }
            other => panic!("Expected ExchangeCommands, got {:?}", other),
        }
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));
    }

    #[test]
    fn test_with_session_show_qr_continue_emits_scan_request() {
        let session = create_test_session();
        let mut engine = ExchangeEngine::with_session(config_no_groups(), session);
        let _ = engine.drain_commands(); // drain initial QrDisplay

        // Press continue → ScanQr
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });

        // Should emit QrRequestScan command
        match result {
            ActionResult::ExchangeCommands { commands } => {
                assert_eq!(commands.len(), 1);
                assert_eq!(
                    commands[0],
                    vauchi_core::exchange::command::ExchangeCommand::QrRequestScan
                );
            }
            other => panic!(
                "Expected ExchangeCommands with QrRequestScan, got {:?}",
                other
            ),
        }
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ScanQr));
    }

    #[test]
    fn test_handle_hardware_event_ble_discovery_emits_connect() {
        let session = create_test_session();
        let mut engine = ExchangeEngine::with_session(config_no_groups(), session);
        let _ = engine.drain_commands();

        // Simulate BLE discovery
        let result = engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::BleDeviceDiscovered {
                id: "device-1".into(),
                rssi: -42,
                adv_data: vec![],
            },
        );

        // Should emit BleConnect command
        assert!(result.is_some(), "expected Some value");
        if let Some(ActionResult::ExchangeCommands { commands }) = result {
            assert!(
                commands.iter().any(|c| matches!(
                    c,
                    vauchi_core::exchange::command::ExchangeCommand::BleConnect { .. }
                )),
                "Expected BleConnect command in {:?}",
                commands
            );
        }
    }

    #[test]
    fn test_without_session_preserves_legacy_behavior() {
        let mut engine = ExchangeEngine::new(config_no_groups());

        // No session
        assert!(engine.session().is_none());

        // ShowQr → ScanQr should return RequestCamera (legacy)
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert!(matches!(result, ActionResult::RequestCamera));

        // handle_hardware_event handles QrScanned via legacy TextChanged path
        let result =
            engine.handle_hardware_event(vauchi_core::exchange::ExchangeHardwareEvent::QrScanned {
                data: "test".into(),
            });
        assert!(
            result.is_some(),
            "QrScanned should be handled even without session"
        );

        // Non-QR events return None without session
        let result = engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::BleDeviceDiscovered {
                id: "d1".into(),
                rssi: -40,
                adv_data: vec![],
            },
        );
        assert!(
            result.is_none(),
            "BLE events should be ignored without session"
        );
    }

    /// Helper: create two sessions (Alice and Bob) and return Alice's engine
    /// plus Bob's QR data string (what Alice would scan).
    fn create_alice_engine_and_bob_qr() -> (ExchangeEngine, String) {
        let alice_identity = vauchi_core::identity::Identity::create("Alice");
        let alice_card = vauchi_core::contact_card::ContactCard::new("Alice");
        let alice_proximity = vauchi_core::exchange::ManualConfirmationVerifier::new();
        let alice_session = vauchi_core::exchange::ExchangeSession::new_qr(
            alice_identity,
            alice_card,
            alice_proximity,
        );

        let bob_identity = vauchi_core::identity::Identity::create("Bob");
        let bob_card = vauchi_core::contact_card::ContactCard::new("Bob");
        let bob_proximity = vauchi_core::exchange::ManualConfirmationVerifier::new();
        let mut bob_session =
            vauchi_core::exchange::ExchangeSession::new_qr(bob_identity, bob_card, bob_proximity);
        // Start Bob's QR so we can get his data string
        bob_session
            .apply(vauchi_core::exchange::ExchangeEvent::StartQR)
            .unwrap();
        let bob_qr = bob_session.qr().unwrap();
        let bob_qr_data = bob_qr.to_data_string();

        let engine = ExchangeEngine::with_session(config_no_groups(), alice_session);
        (engine, bob_qr_data)
    }

    #[test]
    fn test_qr_scanned_advances_step_to_verifying() {
        let (mut engine, bob_qr_data) = create_alice_engine_and_bob_qr();
        let _ = engine.drain_commands(); // drain initial QrDisplay

        // Move to ScanQr
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ScanQr));

        // Simulate scanning Bob's QR
        let result =
            engine.handle_hardware_event(vauchi_core::exchange::ExchangeHardwareEvent::QrScanned {
                data: bob_qr_data,
            });

        // Should advance to Verifying
        assert!(result.is_some(), "expected Some value");
        assert_eq!(
            engine.step,
            ExchangeStep::Qr(QrStep::Verifying),
            "After QrScanned, engine step should be Verifying"
        );
    }

    #[test]
    fn test_show_qr_screen_uses_session_qr_data_when_active() {
        let session = create_test_session();
        let mut engine = ExchangeEngine::with_session(config_no_groups(), session);
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
            vauchi_core::exchange::command::ExchangeCommand::QrDisplay { .. }
        ));

        // 2. User presses "Scan Their Code" → QrRequestScan command
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        match result {
            ActionResult::ExchangeCommands { commands } => {
                assert_eq!(commands.len(), 1);
                assert_eq!(
                    commands[0],
                    vauchi_core::exchange::command::ExchangeCommand::QrRequestScan
                );
            }
            other => panic!("Expected ExchangeCommands, got {:?}", other),
        }

        // 3. Frontend scans Bob's QR → feed as hardware event
        let result =
            engine.handle_hardware_event(vauchi_core::exchange::ExchangeHardwareEvent::QrScanned {
                data: bob_qr_data,
            });
        assert!(result.is_some(), "expected Some value");

        // 4. Engine should be in Verifying step
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::Verifying));

        // 5. Session should be in PeerScanned state
        let session = engine.session().unwrap();
        assert!(
            matches!(
                session.state(),
                vauchi_core::exchange::ExchangeState::PeerScanned { .. }
            ),
            "Session should be in PeerScanned state, got {:?}",
            session.state()
        );
    }

    #[test]
    fn test_selected_groups_persists_through_exchange() {
        let mut engine = ExchangeEngine::new(config_with_groups());

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
        let mut engine = ExchangeEngine::new(config_no_groups());
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
        let mut engine = ExchangeEngine::new(config_no_groups());
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
        let mut engine = ExchangeEngine::new(config_no_groups());
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
        let engine = ExchangeEngine::new(config_mode_selection());
        assert_eq!(engine.step, ExchangeStep::ModeSelection);
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_mode_selection");
    }

    #[test]
    fn mode_preset_skips_mode_selection() {
        let engine = ExchangeEngine::new(config_no_groups());
        // config_no_groups() sets mode = Some(Glance)
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));
    }

    #[test]
    fn mode_selection_pick_advances_to_qr() {
        let mut engine = ExchangeEngine::new(config_mode_selection());
        assert_eq!(engine.step, ExchangeStep::ModeSelection);

        // Pick Glance mode
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:quick".into(),
            item_id: "mode:glance".into(),
        });

        // Should advance past mode selection
        assert_ne!(engine.step, ExchangeStep::ModeSelection);
        // Mode should be stored in config
        assert_eq!(engine.config.mode, Some(ExchangeMode::Glance));
        // Should navigate to QR (no groups in this config)
        assert!(
            matches!(engine.step, ExchangeStep::Qr(QrStep::ShowQr)),
            "Expected Qr(ShowQr), got {:?}",
            engine.step
        );
        assert!(
            matches!(result, ActionResult::NavigateTo(_)),
            "Expected NavigateTo, got {:?}",
            result
        );
    }

    #[test]
    fn mode_selection_pick_with_groups_goes_to_group_selection() {
        let mut config = config_mode_selection();
        config.available_groups = vec![("g1".into(), "Work".into())];
        let mut engine = ExchangeEngine::new(config);
        assert_eq!(engine.step, ExchangeStep::ModeSelection);

        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:quick".into(),
            item_id: "mode:glance".into(),
        });

        assert_eq!(engine.step, ExchangeStep::GroupSelection);
    }

    // ── Hover / Glance mode tests ──────────────────────────────────

    #[test]
    fn hover_mode_routes_through_qr_flow() {
        let mut engine = ExchangeEngine::new(config_mode_selection());

        // Pick Hover
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:standard".into(),
            item_id: "mode:hover".into(),
        });
        assert_eq!(engine.config.mode, Some(ExchangeMode::Hover));
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));

        // Verify QR screen renders
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_show_qr");
    }

    #[test]
    fn glance_mode_routes_through_qr_flow() {
        let mut engine = ExchangeEngine::new(config_mode_selection());

        // Pick Glance
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "category:quick".into(),
            item_id: "mode:glance".into(),
        });
        assert_eq!(engine.config.mode, Some(ExchangeMode::Glance));
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ShowQr));

        // QR flow works identically for both modes at engine level
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert!(matches!(result, ActionResult::RequestCamera));
        assert_eq!(engine.step, ExchangeStep::Qr(QrStep::ScanQr));
    }

    #[test]
    fn field_preview_change_groups_returns_to_group_selection() {
        let mut config = config_mode_selection();
        config.available_groups = vec![("g1".into(), "Work".into())];
        let mut engine = ExchangeEngine::new(config);

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
        let engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            ..config_no_groups()
        });
        assert_eq!(engine.step, ExchangeStep::Link(LinkStep::ShareUrl));
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_share_url");
    }

    // @internal
    #[test]
    fn test_link_mode_share_advances_to_waiting() {
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            ..config_no_groups()
        });
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
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            ..config_no_groups()
        });
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "cancel".into(),
        });
        assert_eq!(result, ActionResult::Complete);
    }

    // @internal
    #[test]
    fn test_link_mode_with_groups_goes_through_preview() {
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            ..config_with_groups()
        });
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
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            ..config_no_groups()
        });
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
        let engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            ..config_no_groups()
        });
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
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            ..config_no_groups()
        });
        let commands = engine.drain_commands();
        assert_eq!(commands.len(), 1, "must emit 1 presence deposit command");
        assert!(matches!(
            &commands[0],
            ExchangeCommand::RelayEscrowDeposit { .. }
        ));
        // Second drain is empty
        assert!(engine.drain_commands().is_empty());
    }

    // @internal
    #[test]
    fn test_link_mode_share_url_screen_shows_generated_url() {
        let engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            ..config_no_groups()
        });
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
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            ..config_no_groups()
        });
        let _ = engine.drain_commands(); // drain presence deposit
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "share".into(),
        });
        match result {
            ActionResult::ExchangeCommands { commands } => {
                assert_eq!(commands.len(), 1);
                assert!(
                    matches!(&commands[0], ExchangeCommand::ShowShareSheet { url } if url.starts_with("vauchi://exchange?")),
                    "Share must emit ShowShareSheet with the link URL"
                );
            }
            other => panic!("Expected ExchangeCommands, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn test_link_shared_event_emits_escrow_check() {
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            ..config_no_groups()
        });
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
        let result =
            engine.handle_hardware_event(vauchi_core::exchange::ExchangeHardwareEvent::LinkShared);
        match result {
            Some(ActionResult::ExchangeCommands { commands }) => {
                assert_eq!(commands.len(), 1);
                assert!(
                    matches!(&commands[0], ExchangeCommand::RelayEscrowCheck { .. }),
                    "LinkShared must trigger escrow check polling"
                );
            }
            other => panic!("Expected ExchangeCommands, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn test_link_escrow_ready_emits_retrieve_and_transitions_to_retrieving() {
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            ..config_no_groups()
        });
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
        let result = engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::RelayEscrowReady { gate_hash: hs_gate },
        );
        // Must transition to Retrieving
        assert_eq!(
            engine.step,
            ExchangeStep::Link(LinkStep::Retrieving),
            "Must transition to Retrieving after handshake gate ready"
        );
        // Must emit RelayEscrowRetrieve with presence_slot (authenticates
        // with OUR slot; relay returns the OTHER slot's blob = responder's epk)
        match result {
            Some(ActionResult::ExchangeCommands { commands }) => {
                assert_eq!(commands.len(), 1);
                if let ExchangeCommand::RelayEscrowRetrieve { slot_hash, .. } = &commands[0] {
                    assert_eq!(
                        slot_hash, &expected_slot,
                        "retrieve must use presence_slot for auth"
                    );
                } else {
                    panic!("expected RelayEscrowRetrieve");
                }
            }
            other => panic!("Expected ExchangeCommands, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn test_link_escrow_failed_shows_error() {
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            ..config_no_groups()
        });
        let _ = engine.drain_commands();
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "share".into(),
        });
        let result = engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::RelayEscrowFailed {
                gate_hash: vec![],
                reason: "gate expired".into(),
            },
        );
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
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            ..config_no_groups()
        });
        let _ = engine.drain_commands();
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "share".into(),
        });
        // Unknown gate_hash — should be ignored (returns None)
        let result = engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::RelayEscrowReady {
                gate_hash: vec![0xAA; 32],
            },
        );
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
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Magic),
            ..config_no_groups()
        });
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
        let mut engine = ExchangeEngine::new(config_no_groups());
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
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Magic),
            ..config_no_groups()
        });
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
            matches!(result, ActionResult::ExchangeCommands { .. }),
            "Expected ExchangeCommands for link setup"
        );
    }

    // @internal
    #[test]
    fn retry_clears_ble_fallback_flag() {
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(ExchangeMode::Bump),
            ..config_no_groups()
        });
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
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            mode: Some(mode),
            ..config_no_groups()
        });
        assert_eq!(engine.step, ExchangeStep::Ble(BleStep::Discovering));

        // Discovery
        let result = engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::BleDeviceDiscovered {
                id: "peer-1".into(),
                rssi: -45,
                adv_data: vec![],
            },
        );
        assert!(result.is_some());
        assert_eq!(engine.step, ExchangeStep::Ble(BleStep::Handshaking));

        // Connection
        let result = engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::BleConnected {
                device_id: "peer-1".into(),
            },
        );
        assert!(result.is_some());
        assert_eq!(engine.step, ExchangeStep::Ble(BleStep::Exchanging));
        engine
    }

    // @internal
    #[test]
    fn magic_full_flow_discovery_to_success() {
        let mut engine = ble_engine_to_exchanging(ExchangeMode::Magic);

        // Card data
        engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::BleCharacteristicNotified {
                uuid: "card".into(),
                data: vec![1, 2, 3],
            },
        );
        // Audio response → proximity done → complete
        let result = engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::AudioResponseReceived {
                data: vec![0xAA],
            },
        );
        assert!(result.is_some());
        assert_eq!(engine.step, ExchangeStep::Success);
    }

    // @internal
    #[test]
    fn bump_full_flow_discovery_to_success() {
        let mut engine = ble_engine_to_exchanging(ExchangeMode::Bump);

        // Card data
        engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::BleCharacteristicNotified {
                uuid: "card".into(),
                data: vec![4, 5, 6],
            },
        );
        // Impact → proximity done → complete
        let result = engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::ImpactDetected {
                timestamp_ms: 100,
                magnitude_milli_g: 3500,
            },
        );
        assert!(result.is_some());
        assert_eq!(engine.step, ExchangeStep::Success);
    }

    // @internal
    #[test]
    fn shake_full_flow_discovery_to_success() {
        let mut engine = ble_engine_to_exchanging(ExchangeMode::Shake);

        // Feed accel samples (triggers recording + envelope send)
        for i in 0..50 {
            engine.handle_hardware_event(
                vauchi_core::exchange::ExchangeHardwareEvent::AccelerometerData {
                    x_milli_g: ((i as f32 * 0.1).sin() * 2000.0) as i32,
                    y_milli_g: ((i as f32 * 0.1).cos() * 1500.0) as i32,
                    z_milli_g: 1000,
                    timestamp_ms: i * 10,
                },
            );
        }

        // Card data
        engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::BleCharacteristicNotified {
                uuid: "card".into(),
                data: vec![7, 8, 9],
            },
        );

        // Peer shake envelope (use encoded constant data for simplicity)
        let peer_envelope = vauchi_core::exchange::shake_protocol::encode_envelope(&[1.5; 50]);
        let result = engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::BleCharacteristicNotified {
                uuid: vauchi_core::exchange::CHAR_DATA_WRITE.into(),
                data: peer_envelope,
            },
        );
        assert!(result.is_some());
        assert_eq!(engine.step, ExchangeStep::Success);
    }

    // @internal
    #[test]
    fn ble_disconnect_during_exchange_offers_relay_fallback() {
        let mut engine = ble_engine_to_exchanging(ExchangeMode::Magic);

        let result = engine.handle_hardware_event(
            vauchi_core::exchange::ExchangeHardwareEvent::BleDisconnected {
                reason: "connection lost".into(),
            },
        );
        assert!(result.is_some());
        assert_eq!(engine.step, ExchangeStep::Failed);
        assert!(engine.ble_fallback_available);

        // Accept fallback → switch to Link
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "fallback_relay".into(),
        });
        assert_eq!(engine.step, ExchangeStep::Link(LinkStep::ShareUrl));
    }
}
