// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Which shell executes a command (ADR-077). The match names every variant,
//! so a new command does not compile until it chooses; a new hardware
//! command is also added to `tests/fixtures/command_targets_v1.json`, which
//! shells route by.

use serde::{Deserialize, Serialize};

use super::Command;

/// The shell a [`Command`] is delivered to.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandTarget {
    /// The app's display shell: presentation, pickers, screen settings.
    Display,
    /// The OS's hardware shell: radios, camera, audio, sensors, network
    /// I/O Core delegates, the keychain and app lifecycle.
    Hardware,
}

impl Command {
    /// The shell that executes this command.
    // DirectSendCard is deprecated (ADR-066) but still decodes, so it keeps
    // a route until the variant is removed.
    #[allow(deprecated)]
    pub fn target(&self) -> CommandTarget {
        match self {
            Self::QrRequestScan
            | Self::ScheduleWakeup { .. }
            | Self::BleStartAdvertising { .. }
            | Self::BleStartScanning { .. }
            | Self::BleConnect { .. }
            | Self::BleWriteCharacteristic { .. }
            | Self::BleReadCharacteristic { .. }
            | Self::BleDisconnect { .. }
            | Self::BleStopScanning
            | Self::NfcActivate { .. }
            | Self::NfcDeactivate
            | Self::NfcSendApdu { .. }
            | Self::AudioEmitChallenge { .. }
            | Self::AudioListenForResponse { .. }
            | Self::AudioStop
            | Self::AccelerometerStart
            | Self::AccelerometerStop
            | Self::RelayEscrowDeposit { .. }
            | Self::RelayEscrowCheck { .. }
            | Self::RelayEscrowRetrieve { .. }
            | Self::DirectSend { .. }
            | Self::DirectSendCard { .. }
            | Self::ImageCaptureFromCamera
            | Self::SwitchCamera { .. }
            | Self::LocationRequest { .. }
            | Self::PostNotification { .. }
            | Self::RequestBiometricUnlock
            | Self::ForgetStoredSecret { .. }
            | Self::CreateUserPresenceSecret { .. }
            | Self::RevealUserPresenceSecret { .. }
            | Self::SaveRestorableState { .. } => CommandTarget::Hardware,
            Self::QrDisplay { .. }
            | Self::ShowShareSheet { .. }
            | Self::ImagePickFromLibrary
            | Self::ImagePickFromFile
            | Self::FilePickFromUser { .. }
            | Self::SetScreenBrightness { .. }
            | Self::SetIdleTimerDisabled { .. }
            | Self::SetOrientationLock { .. }
            | Self::Celebrate { .. }
            | Self::SetContextBar { .. }
            | Self::SetNavigation { .. }
            | Self::PresentOverlay { .. }
            | Self::DismissOverlay { .. }
            | Self::SetPresentationProfile { .. }
            | Self::ReplaceSurface { .. }
            | Self::PresentAlert { .. }
            | Self::ShowToast { .. }
            | Self::OpenExternalUrl { .. }
            | Self::ExportFile { .. }
            | Self::EmitDocument { .. }
            | Self::FinishInvocation { .. }
            | Self::PerformNativeBack
            | Self::ResetApplication
            | Self::ReloadTheme
            | Self::SetApplicationMenu { .. }
            | Self::PickFile { .. }
            | Self::CancelFilePick { .. }
            | Self::RequestPermission { .. } => CommandTarget::Display,
        }
    }
}
