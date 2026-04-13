// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

use crate::notification_types::PendingNotification;
use crate::ui::screen::ScreenModel;
use vauchi_core::exchange::ExchangeCommand;

/// An action the user performed in the UI.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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
    /// User changed a slider value.
    ///
    /// `value_milli` is the new value scaled by 1000 (e.g., -300 for
    /// -0.3). This avoids `f32` in the action enum so `Eq` is
    /// preserved across FFI boundaries.
    SliderChanged {
        component_id: String,
        value_milli: i32,
    },
}

/// The result of handling a user action.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
#[non_exhaustive]
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
    ///
    /// Deprecated per ADR-022 Addendum D: use `ExchangeCommands` with
    /// `ExchangeCommand::QrRequestScan` instead. Will be removed after
    /// all frontends adopt the command/event protocol.
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
    /// App layer should navigate to MyInfo in preview mode for this contact.
    PreviewAs {
        contact_id: String,
    },
    /// App layer should navigate to the Contacts screen (contact picker).
    ShowContactPicker,
    /// App layer should navigate to the fingerprint verification screen.
    VerifyFingerprint {
        contact_id: String,
    },
    /// App layer should open a form dialog (create group, rename, etc.).
    ShowFormDialog {
        dialog_type: String,
        context_id: Option<String>,
    },
    /// The app engine produced OS notifications that should be rendered.
    /// These are non-blocking and do not change the current screen.
    Notify {
        notifications: Vec<PendingNotification>,
    },
}
