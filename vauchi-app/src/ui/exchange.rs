// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange engine — QR-based contact exchange workflow.
//!
//! ADR-031: ExchangeEngine holds an optional `ExchangeSession` to connect
//! the UI workflow with the cryptographic protocol state machine. When a
//! session is provided, transitions emit `ExchangeCommand`s that frontends
//! dispatch to platform hardware (camera, BLE, NFC, audio).

use crate::ui::*;
use vauchi_core::exchange::command::ExchangeCommand;
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
}

#[derive(Clone, Debug, PartialEq)]
enum ExchangeStep {
    /// Pick groups for the new contact (shown only if groups exist).
    GroupSelection,
    ShowQr,
    ScanQr,
    Verifying,
    Success,
    Failed,
}

impl ExchangeStep {
    fn step_number(&self) -> u8 {
        match self {
            Self::GroupSelection => 1,
            Self::ShowQr => 2,
            Self::ScanQr => 3,
            Self::Verifying => 4,
            Self::Success => 5,
            Self::Failed => 6,
        }
    }
}

const TOTAL_STEPS: u8 = 6;

impl ExchangeEngine {
    pub fn new(config: ExchangeConfig) -> Self {
        let step = if config.available_groups.is_empty() {
            ExchangeStep::ShowQr
        } else {
            ExchangeStep::GroupSelection
        };
        Self {
            step,
            config,
            scanned_data: None,
            selected_groups: Vec::new(),
            session: None,
            failure_detail: None,
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
        let step = if config.available_groups.is_empty() {
            ExchangeStep::ShowQr
        } else {
            ExchangeStep::GroupSelection
        };

        // If starting directly at ShowQr, kick off the session now.
        // StartQR should always succeed on a fresh Idle session.
        if step == ExchangeStep::ShowQr {
            if session.apply(ExchangeEvent::StartQR).is_err() {
                // Session failed to start — proceed without a session so the
                // UI still works (falls back to static QR data).
                return Self {
                    step,
                    config,
                    scanned_data: None,
                    selected_groups: Vec::new(),
                    session: None,
                    failure_detail: None,
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
        }
    }

    /// Drains any pending commands from the session (ADR-031).
    ///
    /// Call this after construction with `with_session()` to get the
    /// initial `QrDisplay` command.
    pub fn drain_commands(&mut self) -> Vec<ExchangeCommand> {
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

    fn progress(&self) -> Progress {
        Progress {
            current_step: self.step.step_number(),
            total_steps: TOTAL_STEPS,
            label: None,
        }
    }

    fn build_screen(&self) -> ScreenModel {
        match self.step {
            ExchangeStep::GroupSelection => {
                let items: Vec<ToggleItem> = self
                    .config
                    .available_groups
                    .iter()
                    .map(|(id, name)| ToggleItem {
                        id: id.clone(),
                        label: name.clone(),
                        selected: self.selected_groups.contains(id),
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
                    progress: Some(self.progress()),
                }
            }
            ExchangeStep::ShowQr => {
                // ADR-031: Use session QR data when available (cryptographically
                // generated with ephemeral keys), falling back to config for
                // legacy UI-only mode.
                let qr_data = self
                    .session
                    .as_ref()
                    .and_then(|s| s.qr())
                    .map(|qr| qr.to_data_string())
                    .unwrap_or_else(|| self.config.own_qr_data.clone());

                ScreenModel {
                    screen_id: "exchange_show_qr".into(),
                    title: "Share Your Code".into(),
                    subtitle: None,
                    components: vec![Component::QrCode {
                        id: "own_qr".into(),
                        data: qr_data,
                        mode: QrMode::Display,
                        label: Some(self.config.own_name.clone()),
                    }],
                    actions: vec![ScreenAction {
                        id: "continue".into(),
                        label: "Scan Their Code".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                    }],
                    progress: Some(self.progress()),
                }
            }
            ExchangeStep::ScanQr => ScreenModel {
                screen_id: "exchange_scan_qr".into(),
                title: "Scan Their Code".into(),
                subtitle: None,
                components: vec![Component::QrCode {
                    id: "scan_qr".into(),
                    data: String::new(),
                    mode: QrMode::Scan,
                    label: None,
                }],
                actions: vec![ScreenAction {
                    id: "back".into(),
                    label: "Back".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                }],
                progress: Some(self.progress()),
            },
            ExchangeStep::Verifying => ScreenModel {
                screen_id: "exchange_verifying".into(),
                title: "Verifying".into(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "verifying_status".into(),
                    icon: None,
                    title: "Verifying...".into(),
                    detail: None,
                    status: Status::InProgress,
                }],
                actions: vec![],
                progress: Some(self.progress()),
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
                }],
                actions: vec![ScreenAction {
                    id: "done".into(),
                    label: "Done".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                }],
                progress: Some(self.progress()),
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
            },
        }
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
                self.step = ExchangeStep::Verifying;
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
                }
                self.step = ExchangeStep::ShowQr;
                self.start_session_if_needed()
            }
            (ExchangeStep::ShowQr, UserAction::ActionPressed { action_id })
                if action_id == "continue" =>
            {
                self.step = ExchangeStep::ScanQr;
                // ADR-031: emit QrRequestScan command if session is active
                if self.session.is_some() {
                    ActionResult::ExchangeCommands {
                        commands: vec![ExchangeCommand::QrRequestScan],
                    }
                } else {
                    ActionResult::RequestCamera
                }
            }
            (ExchangeStep::ScanQr, UserAction::ActionPressed { action_id })
                if action_id == "back" =>
            {
                self.step = ExchangeStep::ShowQr;
                ActionResult::NavigateTo(self.build_screen())
            }
            (
                ExchangeStep::ScanQr,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "scanned_data" => {
                self.scanned_data = Some(value);
                self.step = ExchangeStep::Verifying;
                ActionResult::NavigateTo(self.build_screen())
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
                self.step = ExchangeStep::ShowQr;
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
        }
    }

    #[test]
    fn test_no_groups_skips_selection() {
        let engine = ExchangeEngine::new(config_no_groups());
        // Should start directly at ShowQr when no groups available
        assert_eq!(engine.step, ExchangeStep::ShowQr);
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

        // Continue to ShowQr
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert!(matches!(result, ActionResult::NavigateTo(_)));
        assert_eq!(engine.step, ExchangeStep::ShowQr);

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
        assert_eq!(engine.step, ExchangeStep::ShowQr);
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
        assert_eq!(engine.step, ExchangeStep::ShowQr);

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
    fn test_with_session_group_continue_starts_qr() {
        let session = create_test_session();
        let mut engine = ExchangeEngine::with_session(config_with_groups(), session);

        // Continue from group selection → ShowQr
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
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
        assert_eq!(engine.step, ExchangeStep::ShowQr);
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
        assert_eq!(engine.step, ExchangeStep::ScanQr);
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
        assert_eq!(engine.step, ExchangeStep::ScanQr);

        // Simulate scanning Bob's QR
        let result =
            engine.handle_hardware_event(vauchi_core::exchange::ExchangeHardwareEvent::QrScanned {
                data: bob_qr_data,
            });

        // Should advance to Verifying
        assert!(result.is_some(), "expected Some value");
        assert_eq!(
            engine.step,
            ExchangeStep::Verifying,
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
        assert_eq!(engine.step, ExchangeStep::Verifying);

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
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });

        // Continue through ShowQr → ScanQr → Verifying → Success
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "scanned_data".into(),
            value: "their-qr".into(),
        });
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
        assert_eq!(engine.step, ExchangeStep::ShowQr);
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
}
