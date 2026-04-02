// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange engine — QR-based contact exchange workflow.
//!
//! ADR-031: ExchangeEngine holds an optional `ExchangeSession` to connect
//! the UI workflow with the cryptographic protocol state machine. When a
//! session is provided, transitions emit `ExchangeCommand`s that frontends
//! dispatch to platform hardware (camera, BLE, NFC, audio).

use crate::ui::exchange_field_preview::{self, FieldPreviewConfig, FieldPreviewResult};
use crate::ui::exchange_link::{self, LinkActionOutcome, LinkHardwareOutcome, LinkStep};
use crate::ui::exchange_mode_selection::{ModeSelectionEngine, ModeSelectionResult};
use crate::ui::exchange_qr::{self, QrActionOutcome, QrStep};
use crate::ui::*;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::command::ExchangeCommand;
use vauchi_core::exchange::link_mode::{self, LinkInitiation};
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::exchange::{ExchangeEvent, ExchangeSession};

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
            Self::Link(link) => link.step_number(4),
            Self::Success => 4 + QrStep::STEP_COUNT,
            Self::Failed => 5 + QrStep::STEP_COUNT,
        }
    }
}

// mode + group + preview + sub-flow + success/failed
// QR and Link sub-flows must have the same step count for consistent progress.
const _: () = assert!(QrStep::STEP_COUNT == LinkStep::STEP_COUNT);
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
        Self {
            step,
            config,
            scanned_data: None,
            selected_groups: Vec::new(),
            session: None,
            failure_detail: None,
            mode_selection,
            field_preview: None,
            link_initiation,
            pending_link_commands,
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
                    mode_selection,
                    field_preview: None,
                    link_initiation: None,
                    pending_link_commands: Vec::new(),
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
            mode_selection,
            field_preview: None,
            link_initiation: None,
            pending_link_commands: Vec::new(),
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

    /// Handle Link mode hardware events (ADR-031).
    ///
    /// Delegates to `exchange_link::handle_link_hw_event` for protocol logic,
    /// then interprets the outcome (state transitions, command emission).
    fn handle_link_hardware_event(
        &mut self,
        event: vauchi_core::exchange::ExchangeHardwareEvent,
    ) -> Option<ActionResult> {
        let li = self.link_initiation.as_ref()?;
        let outcome = exchange_link::handle_link_hw_event(li, &event)?;
        match outcome {
            LinkHardwareOutcome::PollHandshakeGate { commands } => {
                Some(ActionResult::ExchangeCommands { commands })
            }
            LinkHardwareOutcome::RetrieveFromHandshake { commands } => {
                self.step = ExchangeStep::Link(LinkStep::Retrieving);
                Some(ActionResult::ExchangeCommands { commands })
            }
            LinkHardwareOutcome::Failed { reason } => {
                self.failure_detail = Some(reason);
                self.step = ExchangeStep::Failed;
                Some(ActionResult::UpdateScreen(self.build_screen()))
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
            ExchangeStep::Failed => ScreenModel {
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
                actions: vec![
                    ScreenAction {
                        id: "retry".into(),
                        label: "Retry".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                    },
                    ScreenAction {
                        id: "cancel".into(),
                        label: "Cancel".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                    },
                ],
                progress: Some(self.progress()),
                ..Default::default()
            },
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
        if let Err(e) = session.apply_hardware_event(event) {
            // The session rejected the event (invalid state, malformed QR, etc.).
            // Transition to Failed so the UI reflects the error.
            self.failure_detail = Some(e.user_message().to_string());
            self.step = ExchangeStep::Failed;
            return Some(ActionResult::UpdateScreen(self.build_screen()));
        }
        let commands = session.drain_commands();

        // Sync engine step from session state
        match session.state() {
            vauchi_core::exchange::ExchangeState::Complete { .. } => {
                self.step = ExchangeStep::Success;
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
                    // Skip → go straight to QR (no preview needed)
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
                // Restore to the correct sub-flow based on selected mode
                if self.config.mode == Some(ExchangeMode::Link) {
                    return self.start_link_mode();
                }
                self.step = ExchangeStep::Qr(QrStep::ShowQr);
                ActionResult::NavigateTo(self.build_screen())
            }
            (ExchangeStep::Failed, UserAction::ActionPressed { action_id })
                if action_id == "cancel" =>
            {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
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
}
