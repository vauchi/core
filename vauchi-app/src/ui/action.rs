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
    /// User invoked a per-row action (swipe, long-press, or overflow
    /// menu). `action_id` matches the id on the
    /// [`crate::ui::ListItemAction`] the engine produced for that row.
    ListItemAction {
        component_id: String,
        item_id: String,
        action_id: String,
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
    /// User tapped the (i) info icon on a component.
    /// Core resolves the key to localized help content.
    InfoRequested {
        key: String,
    },
}

/// Classification of the mutation requested by
/// [`ActionResult::ContactAction`]. Mirrors
/// [`crate::ui::ListItemActionKind`] but lives on the result side so the
/// app layer can dispatch without re-parsing action ids.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContactActionKind {
    Archive,
    Unarchive,
    Hide,
    Unhide,
    /// Soft-delete (imported contacts only). The inverse is `Undelete`.
    Delete,
    Undelete,
}

/// Where to navigate after onboarding completes.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PostOnboardingDestination {
    MainScreen,
    Exchange,
    ImportContacts,
    SecurityInfo,
    BackupSetup,
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
    /// Onboarding complete — navigate to the chosen destination.
    CompleteWith {
        destination: PostOnboardingDestination,
    },
    /// Frontend should switch to the device linking flow.
    StartDeviceLink,
    /// Frontend should switch to the backup import flow.
    StartBackupImport,
    /// Frontend should open the contact detail view.
    OpenContact {
        contact_id: String,
    },
    /// App layer should perform a per-row mutation on a contact
    /// (archive/hide/delete and their inverses). The app layer maps this
    /// to the appropriate `Vauchi` call, then shows a toast with an
    /// undo action pointing to the inverse [`ContactActionKind`].
    ContactAction {
        contact_id: String,
        kind: ContactActionKind,
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
    /// Backup export completed — frontend should save or share the data.
    ///
    /// The data is a hex-encoded encrypted backup blob. The frontend
    /// presents a file-save dialog or share sheet.
    BackupExportComplete {
        data: String,
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
    /// Frontend should display a help info overlay.
    /// Rendered as bottom sheet (mobile), popover (desktop),
    /// or inline text (TUI/CLI).
    ShowInfoOverlay {
        title: String,
        body: String,
    },
}
