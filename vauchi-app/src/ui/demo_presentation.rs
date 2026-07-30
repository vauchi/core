// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core-owned presentation reducer used by the browser demo.
//!
//! The demo intentionally exercises isolated workflows without constructing a
//! full platform app. Keeping this adapter in Core ensures the WASM shell still
//! consumes only canonical [`Event`]s and [`Command`]s.

use vauchi_core::{AlertSpec, Command, Event, ExportFileSpec, SurfaceId, ToastSpec};

use super::{
    ActionResult, ContextualSurface, ContextualSurfaceRoute, EmergencyShredEngine,
    LockScreenEngine, OnboardingEngine, PreparedSurface, PresentationCoordinator, ScreenModel,
    TabInfo, UserAction, WorkflowEngine,
};
use crate::i18n::Locale;

/// A small Core reducer that exposes representative workflows to the web demo.
pub struct DemoPresentationEngine {
    engine: Box<dyn WorkflowEngine + Send>,
    revision: u64,
    coordinator: PresentationCoordinator,
    environment_reported: bool,
}

impl DemoPresentationEngine {
    pub fn new(workflow_type: &str) -> Result<Self, String> {
        let engine = engine_for(workflow_type)?;
        let surface_id = surface_id(&engine.current_screen())?;
        Ok(Self {
            engine,
            revision: 0,
            coordinator: PresentationCoordinator::new(surface_id),
            environment_reported: false,
        })
    }

    pub fn initial_commands(&mut self) -> Result<Vec<Command>, String> {
        self.render_next_revision(Vec::new())
    }

    pub fn dispatch(&mut self, event: Event) -> Result<Vec<Command>, String> {
        match event {
            Event::PresentationEnvironmentChanged { .. } => {
                self.environment_reported = true;
                self.coordinator
                    .handle_event(event)
                    .map_err(|error| error.to_string())
            }
            Event::SurfaceActivated { .. } => self
                .coordinator
                .handle_event(event)
                .map_err(|error| error.to_string()),
            Event::ValueChanged { .. }
            | Event::ActionActivated { .. }
            | Event::BackRequested { .. }
            | Event::OverlayDismissed { .. } => self.dispatch_interaction(event),
            hardware_event => {
                let Some(result) = self.engine.handle_hardware_event(hardware_event) else {
                    return Ok(Vec::new());
                };
                self.apply_result(result)
            }
        }
    }

    fn dispatch_interaction(&mut self, event: Event) -> Result<Vec<Command>, String> {
        let screen = self.engine.current_screen();
        let current_surface = surface_id(&screen)?;
        let prepared =
            PreparedSurface::from_screen(current_surface.clone(), self.revision, &screen)
                .map_err(|error| error.to_string())?;
        let contextual = contextual_surface(current_surface, self.revision, &screen)?;

        let route = match &event {
            Event::ValueChanged { .. } => ContextualSurfaceRoute::UserAction(
                prepared.reduce(event).map_err(|error| error.to_string())?,
            ),
            Event::ActionActivated { .. } => match prepared.reduce(event.clone()) {
                Ok(action) => ContextualSurfaceRoute::UserAction(action),
                Err(super::PreparedSurfaceError::UnknownBinding) => contextual
                    .handle_event(event)
                    .map_err(|error| error.to_string())?,
                Err(error) => return Err(error.to_string()),
            },
            _ => contextual
                .handle_event(event)
                .map_err(|error| error.to_string())?,
        };

        match route {
            ContextualSurfaceRoute::Commands(commands) => Ok(commands),
            ContextualSurfaceRoute::UserAction(UserAction::NavigateToTab { action_id }) => {
                self.switch_workflow(&action_id)?;
                self.render_next_revision(Vec::new())
            }
            ContextualSurfaceRoute::UserAction(action) => {
                let result = self.engine.handle_action(action);
                self.apply_result(result)
            }
        }
    }

    fn switch_workflow(&mut self, action_id: &str) -> Result<(), String> {
        let workflow_type = action_id
            .strip_prefix("demo.")
            .ok_or_else(|| "unknown demo navigation interaction".to_owned())?;
        self.engine = engine_for(workflow_type)?;
        Ok(())
    }

    fn apply_result(&mut self, result: ActionResult) -> Result<Vec<Command>, String> {
        let effects = result_effects(result)?;
        self.render_next_revision(effects)
    }

    fn render_next_revision(&mut self, effects: Vec<Command>) -> Result<Vec<Command>, String> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "presentation revision exhausted".to_owned())?;
        let screen = self.engine.current_screen();
        let current_surface = surface_id(&screen)?;
        self.coordinator
            .set_primary_surface(current_surface.clone());

        let prepared =
            PreparedSurface::from_screen(current_surface.clone(), self.revision, &screen)
                .map_err(|error| error.to_string())?;
        let contextual = contextual_surface(current_surface.clone(), self.revision, &screen)?;
        let mut commands = vec![prepared.command()];
        commands.extend(contextual.initial_commands());
        if self.environment_reported {
            commands.extend(
                self.coordinator
                    .handle_event(Event::SurfaceActivated {
                        surface_id: current_surface,
                    })
                    .map_err(|error| error.to_string())?,
            );
        }
        commands.extend(effects);
        Ok(commands)
    }
}

fn engine_for(workflow_type: &str) -> Result<Box<dyn WorkflowEngine + Send>, String> {
    match workflow_type {
        "onboarding" => Ok(Box::new(OnboardingEngine::new())),
        "emergency_shred" => Ok(Box::new(EmergencyShredEngine::new(Locale::English))),
        "lock_screen" => Ok(Box::new(LockScreenEngine::new(3))),
        _ => Err("unknown workflow".to_owned()),
    }
}

fn surface_id(screen: &ScreenModel) -> Result<SurfaceId, String> {
    SurfaceId::new(screen.screen_id.clone()).map_err(|error| error.to_string())
}

fn contextual_surface(
    surface_id: SurfaceId,
    revision: u64,
    screen: &ScreenModel,
) -> Result<ContextualSurface, String> {
    ContextualSurface::compose_revisioned(
        surface_id,
        revision,
        screen,
        &demo_navigation(),
        "Navigate",
        "More actions",
    )
    .map_err(|error| error.to_string())
}

fn demo_navigation() -> Vec<TabInfo> {
    [
        ("onboarding", "Onboarding", "navigation.onboarding"),
        ("emergency_shred", "Emergency Shred", "navigation.shred"),
        ("lock_screen", "Lock Screen", "navigation.lock"),
    ]
    .into_iter()
    .map(|(id, label, icon)| TabInfo {
        id: id.to_owned(),
        action_id: format!("demo.{id}"),
        label: label.to_owned(),
        icon: icon.to_owned(),
        badge_count: 0,
    })
    .collect()
}

fn result_effects(result: ActionResult) -> Result<Vec<Command>, String> {
    let commands = match result {
        ActionResult::UpdateScreen(_)
        | ActionResult::NavigateTo(_)
        | ActionResult::Complete
        | ActionResult::CompleteWith { .. }
        | ActionResult::OnboardingComplete { .. } => Vec::new(),
        ActionResult::PerformNativeBack => vec![Command::PerformNativeBack],
        ActionResult::OpenUrl { url } => vec![Command::OpenExternalUrl { url }],
        ActionResult::ShowAlert { title, message }
        | ActionResult::ShowInfoOverlay {
            title,
            body: message,
        } => vec![Command::PresentAlert {
            alert: AlertSpec { title, message },
        }],
        ActionResult::ShowToast { message, .. } => vec![Command::ShowToast {
            toast: ToastSpec { message },
        }],
        ActionResult::RequestCamera => vec![Command::QrRequestScan],
        ActionResult::BackupExportComplete { data } => vec![Command::ExportFile {
            file: ExportFileSpec {
                suggested_name: "vauchi-backup.vauchi".into(),
                mime_type: "application/octet-stream".into(),
                data: data.into_bytes(),
            },
        }],
        ActionResult::GdprExportComplete { json } => vec![Command::ExportFile {
            file: ExportFileSpec {
                suggested_name: "vauchi-data-export.json".into(),
                mime_type: "application/json".into(),
                data: json.into_bytes(),
            },
        }],
        ActionResult::WipeComplete => vec![Command::ResetApplication],
        ActionResult::Commands { commands } => commands,
        unresolved => {
            return Err(format!(
                "demo workflow emitted unresolved result: {}",
                action_result_name(&unresolved)
            ));
        }
    };
    Ok(commands)
}

fn action_result_name(result: &ActionResult) -> &'static str {
    match result {
        ActionResult::ValidationError { .. } => "ValidationError",
        ActionResult::StartDeviceLink { .. } => "StartDeviceLink",
        ActionResult::OpenContact { .. } => "OpenContact",
        ActionResult::ContactAction { .. } => "ContactAction",
        ActionResult::EditContact { .. } => "EditContact",
        ActionResult::OpenEntryDetail { .. } => "OpenEntryDetail",
        ActionResult::DeviceLinkJoinStart { .. } => "DeviceLinkJoinStart",
        ActionResult::PreviewAs { .. } => "PreviewAs",
        ActionResult::ShowContactPicker => "ShowContactPicker",
        ActionResult::VerifyFingerprint { .. } => "VerifyFingerprint",
        ActionResult::ShowFormDialog { .. } => "ShowFormDialog",
        ActionResult::Notify { .. } => "Notify",
        ActionResult::SetGroupFieldVisibility { .. } => "SetGroupFieldVisibility",
        ActionResult::RetryFailedDeliveries { .. } => "RetryFailedDeliveries",
        ActionResult::StartMultiStageExchange { .. } => "StartMultiStageExchange",
        ActionResult::StartLinkExchange => "StartLinkExchange",
        ActionResult::StartBleExchange { .. } => "StartBleExchange",
        ActionResult::StartNfcExchange => "StartNfcExchange",
        ActionResult::StartDirectTransport => "StartDirectTransport",
        ActionResult::DeviceLinkConfirmManual { .. } => "DeviceLinkConfirmManual",
        ActionResult::DeviceLinkDeny => "DeviceLinkDeny",
        ActionResult::DeviceLinkRetry => "DeviceLinkRetry",
        ActionResult::BiometricUnlockOutcome { .. } => "BiometricUnlockOutcome",
        _ => "Unknown",
    }
}
