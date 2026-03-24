// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

use super::screen::ScreenModel;
use vauchi_core::exchange::ExchangeCommand;

/// A command from the UI layer requesting a Tor backend operation.
///
/// These commands are emitted by `TorSettingsEngine` when the user toggles
/// Tor settings. The app layer dispatches them to the `TorManager` (when
/// the `tor` feature is enabled) or ignores them gracefully.
///
/// This type is unconditionally available (no feature gate) so that the
/// UI layer compiles regardless of whether `tor` is enabled.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TorCommand {
    /// Start the Tor client and establish a circuit.
    Bootstrap,
    /// Shut down the Tor client.
    Shutdown,
    /// Request a new Tor circuit (new exit node).
    RotateCircuit,
    /// Update Tor configuration (e.g., onion preference changed).
    UpdateConfig {
        /// Whether to prefer .onion addresses.
        prefer_onion: bool,
    },
}

/// An action the user performed in the UI.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserAction {
    TextChanged {
        component_id: String,
        value: String,
    },
    ItemToggled {
        component_id: String,
        item_id: String,
    },
    ActionPressed {
        action_id: String,
    },
    FieldVisibilityChanged {
        field_id: String,
        group_id: Option<String>,
        visible: bool,
    },
    GroupViewSelected {
        group_name: Option<String>,
    },
    SearchChanged {
        component_id: String,
        query: String,
    },
    ListItemSelected {
        component_id: String,
        item_id: String,
    },
    SettingsToggled {
        component_id: String,
        item_id: String,
    },
    /// User pressed Undo on a toast with an undo action.
    UndoPressed {
        action_id: String,
    },
}

/// The result of handling a user action.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
pub enum ActionResult {
    UpdateScreen(ScreenModel),
    NavigateTo(ScreenModel),
    ValidationError {
        component_id: String,
        message: String,
    },
    Complete,
    /// Frontend should switch to the device linking flow.
    StartDeviceLink,
    /// Frontend should switch to the backup import flow.
    StartBackupImport,
    /// Frontend should open the contact detail view.
    OpenContact {
        contact_id: String,
    },
    /// Frontend should open the contact edit view.
    EditContact {
        contact_id: String,
    },
    /// Frontend should open the given URL in an external browser.
    OpenUrl {
        url: String,
    },
    /// Frontend should display an alert dialog.
    ShowAlert {
        title: String,
        message: String,
    },
    /// Frontend should open the camera for QR scanning.
    RequestCamera,
    /// Frontend should open the entry detail view for a MyInfo field.
    OpenEntryDetail {
        field_id: String,
    },
    /// Frontend should display a non-blocking toast. Does not replace the current screen.
    ShowToast {
        message: String,
        undo_action_id: Option<String>,
    },
    /// All data has been wiped — frontend should reset to initial state.
    WipeComplete,
    /// Frontend should perform hardware actions for the exchange protocol (ADR-031).
    /// Each command maps to a platform-specific operation (QR display, BLE scan, etc.).
    /// Unsupported commands should be answered with `ExchangeHardwareEvent::HardwareUnavailable`.
    ExchangeCommands {
        commands: Vec<ExchangeCommand>,
    },
    /// Frontend/app layer should dispatch this Tor backend command.
    ///
    /// The app layer forwards these to `TorManager` when the `tor` feature
    /// is enabled, or handles them as no-ops when Tor is not compiled in.
    TorCommand {
        command: TorCommand,
    },
}
