// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The camera in the hardware shell (ADR-077): what a capture is for.

use serde::{Deserialize, Serialize};

/// What a [`super::Command::CameraCapture`] produces.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CameraPurpose {
    /// Decode QR codes until stopped, reporting each as `Event::QrScanned`.
    QrScan,
    /// Take one photo, reported as `Event::ImageReceived`.
    Photo,
}
