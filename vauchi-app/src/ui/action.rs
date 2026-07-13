// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

use crate::notification_types::PendingNotification;
use crate::ui::screen::ScreenModel;
use vauchi_core::BiometricUnlockOutcome;
use vauchi_core::Command;
use vauchi_core::exchange::mode::ExchangeMode;

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
    /// User selected a top-level navigation target (tab bar / sidebar).
    /// `action_id` is the opaque token core minted on the corresponding
    /// `TabInfo.action_id`; the frontend forwards it verbatim and never
    /// constructs or parses it. `AppEngine::handle_action` intercepts this
    /// before per-screen dispatch and resolves it to `NavigateTo`
    /// (ADR-043 Amendment 4: forward navigation is core-resolved). Kept
    /// distinct from `ActionPressed` so global-chrome navigation and
    /// per-screen actions stay on separate dispatch lanes and cannot
    /// collide on `action_id`.
    NavigateToTab {
        action_id: String,
    },
    /// The OS back gesture (Android system BACK / swipe, iOS edge-swipe, ESC).
    /// `AppEngine::handle_action` intercepts it before per-screen dispatch and
    /// owns the decision (ADR-043 Amendment 4: navigation is core-resolved;
    /// ADR-044 Amendment 2a: the frontend forwards it *unconditionally* and
    /// never gates its handler on `can_go_back`). A back step (engine-internal
    /// sub-flow or `nav_history`) pops via `navigate_back()` → `NavigateTo`; a
    /// back-stopping root has nothing to pop → `PerformNativeBack`, and the
    /// frontend performs its platform default (minimize / suspend / no-op).
    NavigateBack,
    FieldVisibilityChanged {
        field_id: String,
        group_id: Option<String>,
        visible: bool,
    },
    VariantSelected {
        variant_id: Option<String>,
    },
    SearchChanged {
        component_id: String,
        query: String,
    },
    ListItemSelected {
        component_id: String,
        item_id: String,
    },
    /// A `vauchi://...` link was opened by the OS, share sheet, or messaging
    /// app. Core parses the URI, validates eligibility, and navigates to the
    /// appropriate screen (exchange consent, device-link join, etc.). Per
    /// ADR-021: frontends forward the raw URI and never construct navigation
    /// targets or decide which flow it belongs to.
    LinkOpened {
        uri: String,
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
    /// The lazy list is approaching the edge of the emitted window — the
    /// renderer asks the engine to re-slice the windowed
    /// [`crate::ui::Component::List`] from `offset`. Only meaningful for
    /// windowed emissions (`total_count > 0`); the engine clamps the
    /// offset, re-slices, and answers with the usual `UpdateScreen`
    /// (`2026-06-11-contacts-list-eager-render-anr` Track B).
    ListWindowRequested {
        component_id: String,
        offset: usize,
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

/// Role the frontend should assume when entering the device-link flow.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DeviceLinkRole {
    /// This device creates the link invitation (device-management
    /// "Link New Device" / the link-initiator UI).
    Initiator,
    /// This device joins an existing link by scanning an invitation
    /// (onboarding or device-replacement "I have my old device").
    Responder,
}

/// The result of handling a user action.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[must_use]
#[non_exhaustive]
pub enum ActionResult {
    UpdateScreen(ScreenModel),
    NavigateTo(ScreenModel),
    /// The back gesture reached a back-stopping root: there is no screen to
    /// pop, so the frontend performs the platform's native back default
    /// (Android `moveTaskToBack` / minimize; iOS suspend / no-op; desktop
    /// no-op). Core owns the "nothing to pop" decision; the frontend owns
    /// only the native mechanism. Retires the frontend `can_go_back` gate on
    /// its system-back handler (ADR-044 Amendment 2a).
    PerformNativeBack,
    ValidationError {
        component_id: String,
        message: String,
    },
    Complete,
    /// Onboarding complete — navigate to the chosen destination.
    CompleteWith {
        destination: PostOnboardingDestination,
    },
    /// Onboarding is finished; the engine has already navigated to the
    /// chosen post-onboarding screen. Frontends should flip their app state
    /// from "onboarding" to "ready" and render the current screen.
    /// Replaces frontend-side enumeration of onboarding `screen_id`s
    /// (`2026-07-06-mobile-domain-shell-violations` I7/A13).
    OnboardingComplete {
        destination: PostOnboardingDestination,
    },
    /// Frontend should switch to the device linking flow in the given role.
    StartDeviceLink {
        role: DeviceLinkRole,
    },
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
    /// Deprecated per ADR-022 Addendum D: use `Commands` with
    /// `Command::QrRequestScan` instead. Will be removed after
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
    /// GDPR data export completed — frontend should save or share the
    /// JSON (file dialog / share sheet). Mirrors `BackupExportComplete`;
    /// the payload is the serialized `export_all_data` result.
    GdprExportComplete {
        json: String,
    },
    /// All data has been wiped — frontend should reset to initial state.
    WipeComplete,
    /// App layer should start the device-link join (responder) machine with
    /// the given device name. Emitted by `DeviceLinkJoinEngine` when the user
    /// confirms the name. Core builds the responder, posts the request on the
    /// next poll tick, and advances through confirmation → adoption.
    DeviceLinkJoinStart {
        device_name: String,
    },
    /// Frontend should perform hardware actions for the exchange protocol (ADR-031).
    /// Each command maps to a platform-specific operation (QR display, BLE scan, etc.).
    /// Unsupported commands should be answered with `Event::HardwareUnavailable`.
    Commands {
        commands: Vec<Command>,
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
    /// App layer should persist a per-field visibility change for a group
    /// (a.k.a. visibility label). Emitted by `GroupDetailEngine` when the
    /// user toggles a field in the visibility list. AppEngine routing
    /// applies the change via
    /// `vauchi.set_group_field_visibility_and_repropagate` and re-emits
    /// the screen so any downstream count (Visible Fields) refreshes.
    SetGroupFieldVisibility {
        group_id: String,
        field_id: String,
        visible: bool,
    },
    /// App layer should requeue every failed delivery in the list for an
    /// immediate retry attempt. Emitted by `DeliveryStatusEngine` when the
    /// user taps the "Retry Failed" footer button. AppEngine routing
    /// iterates the message ids and calls
    /// `vauchi.storage().retries().update_retry_next_time(id, now)` for each (mirror
    /// of `mobile_delivery::manual_retry`), then emits a `ShowToast` with
    /// the count of rescheduled messages.
    RetryFailedDeliveries {
        message_ids: Vec<String>,
    },
    /// App layer should navigate to the multi-stage face-to-face
    /// exchange screen (`AppScreen::MultiStageExchange`).
    ///
    /// Emitted by `ExchangeEngine` when the user picks a mode whose
    /// implementation is the new core-driven multi-stage protocol.
    /// Pair 4 of `2026-04-28-pure-humble-ui-retire-native-screens`
    /// graduated `ExchangeMode::Glance`; Phase 1.E of
    /// `2026-05-11-hover-graduation-plan.md` extended the handoff
    /// to `ExchangeMode::Hover`. The `mode` payload tells AppEngine
    /// which engine constructor to use
    /// (`MultiStageExchangeEngine::new_hover()` vs
    /// `::new_glance()`) — Hover defaults to the front camera and
    /// runs the autonomous audio-handshake trigger; Glance stays
    /// back-camera + audio-quiet.
    ///
    /// Frontends never see this — `PlatformAppEngine` routes it to the
    /// dedicated screen which then auto-creates the
    /// `MobileMultiStageSession` and binds the bridge listener.
    StartMultiStageExchange {
        mode: ExchangeMode,
    },
    /// App layer should navigate to the link-mode initiator screen
    /// (`AppScreen::LinkExchange`).
    ///
    /// Emitted by `ExchangeEngine` when the user picks
    /// `ExchangeMode::Link` from the mode list, and by
    /// `LinkExchangeEngine` itself on the failed screen's Retry action.
    /// AppEngine routing navigates to the dedicated screen, whose factory
    /// builds a fresh `LinkExchangeEngine` and the AppEngine
    /// link-initiator lifecycle constructs the engine-owned
    /// `LinkInitiatorSession`. Frontends never see this — the relay escrow
    /// lifecycle is core/event-driven (no platform bridge).
    StartLinkExchange,
    /// App layer should navigate to the dedicated BLE-exchange screen
    /// (`AppScreen::BleExchange { mode }`) for Magic/Bump/Shake. The screen
    /// factory builds a fresh `BleExchangeEngine`; the legacy
    /// `ExchangeStep::Ble` sub-flow is retired in slice 3. Per
    /// `2026-05-11-ble-exchange-engine-graduation`.
    StartBleExchange {
        mode: ExchangeMode,
    },
    /// App layer should navigate to the dedicated NFC-exchange screen
    /// (`AppScreen::NfcExchange`) for TapTap. The screen factory builds a
    /// fresh `NfcExchangeEngine` (reconstructing the signing `Identity` from
    /// storage). Emitted by `ExchangeEngine` when the user picks
    /// `ExchangeMode::TapTap`, and by `NfcExchangeEngine` itself on the failed
    /// screen's Retry action (a fresh engine re-provisions the consumed,
    /// un-cloneable `Identity`). The legacy `ExchangeStep::Nfc` /
    /// `NfcRoleSelection` sub-flow is retired in a follow-up slice. Per
    /// `2026-05-11`-era exchange-engine graduation program.
    StartNfcExchange,
    /// App layer should navigate to the dedicated DirectTransport (Cable/USB)
    /// exchange screen (`AppScreen::DirectTransport`). The screen factory builds
    /// a fresh `DirectTransportEngine` (re-provisioning the consumed `Identity`).
    /// Emitted by `ExchangeEngine` when the user picks `ExchangeMode::Cable`, and
    /// by `DirectTransportEngine` itself on the failed screen's Retry action. The
    /// legacy `ExchangeStep::DirectTransport` sub-flow is retired in slice 3. Per
    /// `2026-05-11-direct-transport-engine-graduation`.
    StartDirectTransport,
    /// App layer should call `MobileDeviceLinkSession::confirm_manual`
    /// with the given confirmation code and the current unix timestamp.
    ///
    /// Emitted by `DeviceLinkingEngine` from the `ConfirmingDevice` step
    /// when the user taps "codes match" — the single confirmation since
    /// M5 B2b collapsed the redundant proximity screen. Pair 5 of
    /// `2026-04-28-pure-humble-ui-retire-native-screens`. Ultrasonic
    /// confirmation flows through ADR-031 hardware events instead.
    DeviceLinkConfirmManual {
        code: String,
    },
    /// App layer should call `MobileDeviceLinkSession::deny`. Emitted
    /// by `DeviceLinkingEngine` from the `ConfirmingDevice` step when
    /// the user denies the request. The session's cycle thread fires
    /// `on_failed("user_denied")` followed by `on_session_ended()`.
    DeviceLinkDeny,
    /// App layer should create a fresh `MobileDeviceLinkSession`
    /// (single-shot — sessions cannot be reused) and call `start()`.
    /// Emitted by `DeviceLinkingEngine` from `QrExpired` and
    /// `LinkFailed` when the user taps Retry. The engine is already
    /// in the `QrPending` state when this is emitted; the new session
    /// will fire `on_qr_ready` to advance it.
    DeviceLinkRetry,
    /// Outcome of the ADR-031 biometric-unlock hardware event
    /// ([`vauchi_core::Event::BiometricUnlockSucceeded`]). `Unlocked`
    /// proceeds straight to the post-auth screen; `PromptForDuressPin`
    /// tells the frontend to present the PIN entry screen so the
    /// subsequent `authenticate()` call can resolve Normal vs Duress.
    /// Replaces the legacy `PlatformAppEngine::biometric_unlock_check`
    /// typed getter (Track B of
    /// `2026-05-11-pure-functional-core-program`).
    BiometricUnlockOutcome {
        outcome: BiometricUnlockOutcome,
    },
}
