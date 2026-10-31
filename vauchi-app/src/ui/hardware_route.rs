// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Delivery of hardware commands to the OS's hardware shell (ADR-077). A
//! hardware shell attaches by reporting the device's capabilities; until a
//! platform has one, its display shell keeps receiving every command. The
//! report is kept, so a presentation shown before storage opens can hand it
//! to the engine that opens afterwards.
//!
//! Once attached, a camera request becomes a viewport for the display shell
//! and a capture for the hardware shell, joined by one preview id. Core
//! stops the capture when the surface that started it is replaced, so the
//! camera never outlives its screen.

use vauchi_core::exchange_types::DeviceCapabilities;
use vauchi_core::{CameraPurpose, Command, CommandTarget, SurfaceId};

#[derive(Debug, Default)]
pub(crate) struct HardwareRoute {
    report: Option<DeviceCapabilities>,
    queue: Vec<Command>,
    current_surface: Option<SurfaceId>,
    capture: Option<Capture>,
    captures_started: u64,
}

#[derive(Debug)]
struct Capture {
    preview_id: String,
    purpose: CameraPurpose,
    /// `None` until the batch that started it has shown its surface.
    surface: Option<SurfaceId>,
}

impl HardwareRoute {
    pub(crate) fn attach(&mut self, capabilities: DeviceCapabilities) {
        self.report = Some(capabilities);
    }

    pub(crate) fn report(&self) -> Option<&DeviceCapabilities> {
        self.report.as_ref()
    }

    /// Keeps hardware commands out of a display batch once a hardware shell
    /// is attached, queuing them for [`Self::drain`].
    pub(crate) fn route(&mut self, commands: Vec<Command>) -> Vec<Command> {
        if self.report.is_none() {
            return commands;
        }
        let mut routed = Vec::with_capacity(commands.len());
        for command in commands {
            match command {
                Command::ReplaceSurface { surface } => {
                    self.surface_shown(&surface.surface_id, &mut routed);
                    routed.push(Command::ReplaceSurface { surface });
                }
                Command::QrRequestScan => self.start_capture(CameraPurpose::QrScan, &mut routed),
                Command::ImageCaptureFromCamera => {
                    self.start_capture(CameraPurpose::Photo, &mut routed);
                }
                other => routed.push(other),
            }
        }
        if let Some(capture) = self.capture.as_mut()
            && capture.surface.is_none()
        {
            capture.surface = self.current_surface.clone();
        }
        let (hardware, display): (Vec<_>, Vec<_>) = routed
            .into_iter()
            .partition(|command| command.target() == CommandTarget::Hardware);
        self.queue.extend(hardware);
        display
    }

    /// A photo is one shot: its answer ends the capture.
    pub(crate) fn photo_answered(&mut self) {
        if self
            .capture
            .as_ref()
            .is_some_and(|capture| capture.purpose == CameraPurpose::Photo)
            && let Some(capture) = self.capture.take()
        {
            self.queue.push(Command::CameraStop {
                preview_id: capture.preview_id,
            });
        }
    }

    pub(crate) fn drain(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.queue)
    }

    fn surface_shown(&mut self, surface_id: &SurfaceId, routed: &mut Vec<Command>) {
        if let Some(capture) = self.capture.as_mut() {
            match &capture.surface {
                None => capture.surface = Some(surface_id.clone()),
                Some(started_on) if started_on != surface_id => self.stop_capture(routed),
                Some(_) => {}
            }
        }
        self.current_surface = Some(surface_id.clone());
    }

    fn start_capture(&mut self, purpose: CameraPurpose, routed: &mut Vec<Command>) {
        self.stop_capture(routed);
        self.captures_started += 1;
        let preview_id = format!("camera-{}", self.captures_started);
        routed.push(Command::CameraViewport {
            preview_id: preview_id.clone(),
        });
        routed.push(Command::CameraCapture {
            preview_id: preview_id.clone(),
            purpose,
        });
        self.capture = Some(Capture {
            preview_id,
            purpose,
            surface: None,
        });
    }

    fn stop_capture(&mut self, routed: &mut Vec<Command>) {
        if let Some(capture) = self.capture.take() {
            routed.push(Command::CameraStop {
                preview_id: capture.preview_id,
            });
        }
    }
}
