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
use vauchi_core::{
    CameraPurpose, Command, CommandTarget, PresentationNode, PresentationQrPurpose, SurfaceId,
};

#[derive(Debug, Default)]
pub(crate) struct HardwareRoute {
    report: Option<DeviceCapabilities>,
    queue: Vec<Command>,
    current_surface: Option<SurfaceId>,
    capture: Option<Capture>,
    captures_started: u64,
    /// A camera request met a report with no camera (private#1070). Read
    /// by the engine's toast, which exists with network-rustls.
    #[cfg_attr(not(feature = "network-rustls"), allow(dead_code))]
    camera_missing: bool,
}

#[derive(Debug)]
struct Capture {
    preview_id: String,
    // Read only by the engine's photo stop, which exists with network-rustls;
    // the lock screen routes without it (VRS03 no-default build).
    #[cfg_attr(not(feature = "network-rustls"), allow(dead_code))]
    purpose: CameraPurpose,
    /// `None` until the batch that started it has shown its surface.
    surface: Option<SurfaceId>,
    /// Started because the surface offers a scan, not by a request: it
    /// ends when the surface stops offering one.
    offered: bool,
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
                    let surface_id = surface.surface_id.clone();
                    let offers_scan = offers_capture(&surface.nodes);
                    routed.push(Command::ReplaceSurface { surface });
                    self.scan_offered(surface_id, offers_scan, &mut routed);
                }
                Command::QrRequestScan | Command::ImageCaptureFromCamera
                    if !self.report.as_ref().is_some_and(|report| report.has_camera) =>
                {
                    // The hardware shell has no camera port: a capture would
                    // be Unsupported and report nothing (private#1070).
                    self.camera_missing = true;
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
    #[cfg(feature = "network-rustls")]
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

    /// The hardware shell releases the camera once it reads a code
    /// (windows-hw `QrCamera`, linux-hw `camera.rs`): the scan is over, and
    /// the next scan screen starts a fresh capture.
    #[cfg(feature = "network-rustls")]
    pub(crate) fn code_scanned(&mut self) {
        if self
            .capture
            .as_ref()
            .is_some_and(|capture| capture.purpose == CameraPurpose::QrScan)
        {
            self.capture = None;
        }
    }

    /// Whether a camera request since the last call found no camera.
    #[cfg(feature = "network-rustls")]
    pub(crate) fn take_camera_missing(&mut self) -> bool {
        std::mem::take(&mut self.camera_missing)
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

    /// A surface that offers a scan gets the camera when the hardware shell
    /// reported one, as a phone's display shell opens its own for it
    /// (private#772); one the person asked for keeps running. Without a
    /// camera nothing is said: the screen only offers, and its paste field
    /// still works.
    fn scan_offered(
        &mut self,
        surface_id: SurfaceId,
        offers_scan: bool,
        routed: &mut Vec<Command>,
    ) {
        let has_camera = self.report.as_ref().is_some_and(|report| report.has_camera);
        match self.capture.as_ref() {
            None if offers_scan && has_camera => {
                self.start_capture(CameraPurpose::QrScan, routed);
                if let Some(capture) = self.capture.as_mut() {
                    capture.surface = Some(surface_id);
                    capture.offered = true;
                }
            }
            Some(capture) if capture.offered && !offers_scan => self.stop_capture(routed),
            _ => {}
        }
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
            offered: false,
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

/// Whether any node, nested in groups or list rows, is a QR code to capture.
fn offers_capture(nodes: &[PresentationNode]) -> bool {
    nodes.iter().any(|node| match node {
        PresentationNode::Qr { purpose, .. } => *purpose == PresentationQrPurpose::Capture,
        PresentationNode::Group { children, .. } => offers_capture(children),
        PresentationNode::List { rows, .. } => rows.iter().any(|row| offers_capture(&row.controls)),
        _ => false,
    })
}
