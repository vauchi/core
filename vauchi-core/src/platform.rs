// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Platform command/event protocol (ADR-031).
//!
//! Core emits [`Command`]s to tell frontends what platform actions to
//! perform: hardware (display QR, start BLE scan, emit audio challenge),
//! IO (file picker, image picker, share sheet), and screen presentation
//! (brightness, idle timer, orientation lock).
//!
//! Frontends report results back via [`Event`]s.
//!
//! Originally introduced under ADR-031 §Hardware as `Command` /
//! `Event` for peer-to-peer exchange transports. The
//! protocol grew past that scope (avatar editor, backup import, screen
//! presentation) — renamed 2026-05-04 to drop the misleading
//! domain prefix. See investigation
//! `_private/docs/investigations/2026-05-04-exchange-command-naming-categorization.md`.
//!
//! Three-axis boundary protocol:
//! - [`crate::ui::UserAction`] — user → core
//! - [`Command`] — core → platform
//! - [`Event`] — platform → core
//!
//! Decouples the protocol state machines from platform-specific APIs
//! and works over UniFFI / C ABI boundaries (all types are serializable).

use serde::{Deserialize, Serialize};

mod presentation;

pub use presentation::{
    AccessibilitySpec, ActionSpec, ActionTone, AlertSpec, BindingId, ChoiceOption, ContextBar,
    ExportFileSpec, InputMode, InputValue, InteractionId, MotionPreference, NotificationSpec,
    NotificationUrgency, OverlayKind, OverlaySpec, PaneLayout, PresentationAxis,
    PresentationIdError, PresentationImageShape, PresentationInputKind, PresentationNode,
    PresentationPaging, PresentationProfile, PresentationQrPurpose, PresentationRow,
    PresentationTextStyle, PresentationTokens, PresentationTone, StandardShortcut, SurfaceId,
    SurfaceLayout, SurfaceSpec, ToastSpec, WindowClass,
};

/// A command from core to the frontend requesting a hardware action.
///
/// Frontends match on these and dispatch to platform-specific APIs
/// (camera, BLE stack, NFC reader, audio subsystem). Commands that the
/// platform cannot fulfil should be answered with
/// [`Event::HardwareUnavailable`].
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Command {
    /// Display a QR code containing `data`.
    QrDisplay { data: String },
    /// Request the frontend to open a QR scanner (camera).
    QrRequestScan,

    /// Core-computed wakeup schedule (ADR-030/031 command + ADR-044 Am2a:
    /// generic timer *data*, not a per-feature flag). Core stays the single
    /// authority on *when* the app-level heartbeat is due; the humble shell
    /// translates this into its platform wakeup — a desktop in-process
    /// interval/thread, iOS `BGAppRefreshTask`, Android `WorkManager` — and,
    /// when it fires, calls `AppEngine::on_wakeup()`, which runs due work and
    /// emits the *next* `ScheduleWakeup`. All fields are relative seconds from
    /// now (the OS APIs take intervals, and it frees the shell from reading
    /// core's clock). Every interval is a *hint*: the OS may fire late,
    /// coalesce, or skip — `on_wakeup` is elapsed-based and idempotent, so a
    /// missed or doubled wake is safe.
    ScheduleWakeup {
        /// Earliest the shell should wake core (never earlier — battery floor).
        earliest_secs: u32,
        /// Latest the work should ideally run by; the shell picks a
        /// higher-fidelity mechanism if this is tight.
        deadline_secs: u32,
        /// Minimum spacing between wakes the shell should honor (coalesce).
        min_interval_secs: u32,
    },

    /// Start advertising the vauchi BLE service with the given payload.
    BleStartAdvertising {
        service_uuid: String,
        payload: Vec<u8>,
    },
    /// Start scanning for vauchi BLE peripherals.
    BleStartScanning { service_uuid: String },
    /// Connect to a discovered BLE device.
    BleConnect { device_id: String },
    /// Write data to a BLE characteristic on the link to `device_id`.
    ///
    /// `device_id` addresses the target link so shells holding more than
    /// one live link (glare) route the write to the right peer instead
    /// of an implicit "current" connection
    /// (`_private/docs/designs/2026-07-22-role-tiebreak-and-glare-design.md`).
    BleWriteCharacteristic {
        device_id: String,
        direction: BleLinkDirection,
        uuid: String,
        data: Vec<u8>,
    },
    /// Read data from a BLE characteristic on the link to `device_id`.
    BleReadCharacteristic {
        device_id: String,
        direction: BleLinkDirection,
        uuid: String,
    },
    /// Disconnect one specific BLE link, addressed by peer and direction.
    ///
    /// Targeted (not "disconnect everything") so glare resolution can drop
    /// exactly the losing link while the surviving link keeps carrying the
    /// handshake.
    BleDisconnect {
        device_id: String,
        direction: BleLinkDirection,
    },

    /// Activate the NFC interface and prepare to exchange `payload`.
    NfcActivate { payload: Vec<u8> },
    /// Deactivate the NFC interface.
    NfcDeactivate,
    /// Send a continuation APDU on the already-active NFC session.
    /// Used by the multi-phase handshake after the initial
    /// `NfcActivate`: the frontend keeps the tag connection alive and
    /// transmits `data` opaquely, then surfaces the peer's response as
    /// `Event::NfcDataReceived`.
    NfcSendApdu { data: Vec<u8> },

    /// Emit ultrasonic PCM samples encoding a challenge.
    ///
    /// Core has already FSK-encoded the challenge bytes; the frontend
    /// just plays the samples through the device speaker. Mono float
    /// PCM at `sample_rate`.
    AudioEmitChallenge { samples: Vec<f32>, sample_rate: u32 },
    /// Listen for an ultrasonic response within `timeout_ms`.
    ///
    /// `sample_rate` is the suggested capture rate; if the device's
    /// preferred rate differs, the frontend reports its actual rate
    /// in [`Event::AudioSamplesRecorded`].
    AudioListenForResponse { timeout_ms: u64, sample_rate: u32 },
    /// Stop all audio operations.
    AudioStop,

    /// Start accelerometer sampling for proximity verification.
    AccelerometerStart,
    /// Stop accelerometer sampling.
    AccelerometerStop,

    /// Deposit encrypted card into relay escrow gate.
    RelayEscrowDeposit {
        gate_hash: Vec<u8>,
        slot_hash: Vec<u8>,
        encrypted_card: Vec<u8>,
        ttl_seconds: u32,
    },
    /// Check relay escrow gate readiness (poll until ready).
    ///
    /// Frontends should poll at `suggested_interval_ms` with exponential
    /// backoff (cap at 5 min). Report `RelayEscrowReady` when gate has
    /// ≥2 deposits, or `RelayEscrowFailed` on error/timeout.
    RelayEscrowCheck {
        gate_hash: Vec<u8>,
        /// Suggested initial polling interval in milliseconds.
        suggested_interval_ms: u32,
    },
    /// Retrieve blob from relay escrow gate.
    RelayEscrowRetrieve {
        gate_hash: Vec<u8>,
        slot_hash: Vec<u8>,
    },

    /// Show system share sheet with a URL.
    ShowShareSheet { url: String },

    // Appended (not inserted) to preserve serde discriminant ordering.
    /// Stop BLE scanning (saves battery after discovery completes).
    BleStopScanning,

    // Appended to preserve serde discriminant ordering.
    /// Send an exchange payload over a direct transport (USB cable / local TCP).
    ///
    /// The frontend should:
    /// 1. Send `payload` to the peer over the established TCP connection
    /// 2. Receive the peer's payload from the same connection
    /// 3. Report the peer's data via [`Event::DirectPayloadReceived`]
    ///
    /// The `is_initiator` flag determines send/recv ordering to avoid deadlock
    /// (initiator sends first, responder receives first).
    DirectSend {
        payload: Vec<u8>,
        is_initiator: bool,
    },

    // ── Image picking (avatar editor, ADR-042) ─────────────────────
    // Appended to preserve serde discriminant ordering.
    /// Request the frontend to open the device photo library / gallery.
    ///
    /// Frontend should return [`Event::ImageReceived`]
    /// with the selected image bytes, or
    /// [`Event::ImagePickCancelled`] if the user
    /// dismisses the picker.
    ImagePickFromLibrary,
    /// Request the frontend to capture an image from the device camera.
    ///
    /// Frontend should return [`Event::ImageReceived`]
    /// with the captured image bytes, or
    /// [`Event::ImagePickCancelled`] if the user
    /// cancels.
    ImageCaptureFromCamera,
    /// Request the frontend to open a file picker for image files.
    ///
    /// Used on desktop platforms where a photo library may not exist.
    /// Frontend should return [`Event::ImageReceived`]
    /// with the selected image bytes, or
    /// [`Event::ImagePickCancelled`] if the user
    /// cancels.
    ImagePickFromFile,

    // Appended to preserve serde discriminant ordering.
    /// Switch the active camera between front- and rear-facing.
    ///
    /// Used by the multi-stage face-to-face exchange screen so the user
    /// can flip the scanner orientation without the frontend owning the
    /// preference. `use_front == true` selects the front camera.
    SwitchCamera { use_front: bool },

    // ── File picking (vCard / backup import, ADR-031) ──────────────
    // Appended to preserve serde discriminant ordering.
    /// Request the frontend to open a file picker.
    ///
    /// `accepted_mime_types` is advisory — frontends may default to a
    /// coarser superset on platforms where the OS picker doesn't filter
    /// by MIME (e.g., older Android versions). `purpose` lets the
    /// frontend label the picker dialog without hardcoding strings;
    /// label text comes from core's locale store via `t(key)`.
    ///
    /// Frontend should return [`Event::FilePickedFromUser`]
    /// with the selected file's bytes, or
    /// [`Event::FilePickCancelledByUser`] if the user
    /// dismisses the picker.
    ///
    /// Distinct from [`Command::ImagePickFromFile`]: that variant
    /// returns [`Event::ImageReceived`] which is shaped
    /// for avatar normalization. File picking returns raw bytes plus
    /// filename for arbitrary payloads (vCard, encrypted backup blob,
    /// future key bundles, etc.).
    FilePickFromUser {
        accepted_mime_types: Vec<String>,
        purpose: FilePickPurpose,
    },

    // Appended to preserve serde discriminant ordering.
    /// Set the device screen brightness, optionally restoring the
    /// platform default when `level` is `None`.
    ///
    /// Used by screens that need a specific brightness for their
    /// hardware to function (e.g., the multi-stage face-to-face
    /// exchange uses 65% brightness so the front camera is not
    /// over-exposed when scanning a peer's QR). The frontend is
    /// responsible for snapshotting the prior value on the *first*
    /// `Some(level)` after a `None` (or app start) so the
    /// subsequent `None` correctly restores it.
    ///
    /// Frontends that have no programmatic brightness control (e.g.,
    /// desktop, where the OS owns it) answer with
    /// [`Event::HardwareUnavailable { transport: "screen_brightness" }`]
    /// — core should treat that as "request honoured at platform
    /// default" and not retry.
    ///
    /// Per `2026-05-01-screen-id-metadata-in-core` cousin
    /// `2026-05-04-exchange-command-screen-presentation`, ADR-031
    /// §Hardware. Phase 1 of the FaceToFaceExchangeView retirement.
    SetScreenBrightness { level: Option<f32> },

    /// Disable or re-enable the platform's idle / auto-lock timer.
    ///
    /// `disabled = true` keeps the screen awake (used by the
    /// multi-stage exchange so a longer-than-30s handshake doesn't
    /// trigger the device's auto-lock). `disabled = false` restores
    /// the platform default on screen exit. Idempotent — frontends
    /// MAY ignore a redundant set/clear.
    ///
    /// Frontends that have no programmatic idle-timer control answer
    /// with [`Event::HardwareUnavailable { transport:
    /// "idle_timer" }`].
    ///
    /// Phase 1 of the FaceToFaceExchangeView retirement (companion
    /// to [`Command::SetScreenBrightness`]).
    SetIdleTimerDisabled { disabled: bool },

    /// Lock or unlock the device's screen orientation.
    ///
    /// `Some(orientation)` pins the screen to the requested orientation
    /// (e.g., `Portrait` so the multi-stage face-to-face exchange QR /
    /// camera layout stays stable while the user is moving the device);
    /// `None` restores the platform default (typically follows the
    /// device's physical rotation).
    ///
    /// Frontends that don't programmatically own orientation
    /// (`linux-gtk`, `linux-qt`, `windows`, desktop `macos` — windowed
    /// apps don't lock device rotation) answer with
    /// [`Event::HardwareUnavailable { transport: "orientation_lock" }`].
    ///
    /// Phase 2c of `2026-05-04-exchange-command-screen-presentation` —
    /// retires the orientation `DisposableEffect` in
    /// `android/app/src/main/kotlin/app/vauchi/ui/FaceToFaceExchangeScreen.kt`.
    SetOrientationLock { orientation: Option<Orientation> },

    // Appended to preserve serde discriminant ordering.
    /// Send our AEAD-encrypted `ContactCard` to the peer over the established
    /// USB / direct-TCP connection (the second leg of the wired exchange, after
    /// the key-bearing [`Command::DirectSend`] / [`Event::DirectPayloadReceived`]
    /// round). `ciphertext` is our card encrypted under a key HKDF-derived from
    /// the agreed `shared_key` (ADR-019 XChaCha20-Poly1305, ADR-007 domain
    /// separation). The frontend sends it and reports the peer's encrypted card
    /// via [`Event::DirectCardReceived`]. `is_initiator` keeps the same
    /// send/recv ordering discriminator as `DirectSend` (avoids TCP deadlock).
    DirectSendCard {
        ciphertext: Vec<u8>,
        is_initiator: bool,
    },
    /// Request the device's current location (ADR-051 contact annotations):
    /// used at an in-person exchange to record *where we met*. The frontend
    /// resolves permission and replies with [`Event::LocationResult`], or
    /// [`Event::PermissionDenied { transport: "location" }`] if the user
    /// declines, or [`Event::HardwareUnavailable`] if there is no location
    /// provider. `timeout_ms` bounds how long the frontend waits for a fix.
    LocationRequest { timeout_ms: u32 },

    // ── Feedback (exchange ceremony, M2 S4) ────────────────────────
    // Appended to preserve serde discriminant ordering.
    /// Render the exchange-success ceremony: a celebratory haptic + sound +
    /// animation (`designs/2026-06-06-exchange-ceremony-design.md`). The
    /// tokens are closed intent enums looked up against a static per-platform
    /// table (Wire-Humble kind rule) — core never sends assets or frequencies.
    /// Frontends skip any axis they can't render; the others still play.
    /// Emitted exactly once per validated exchange success, never on failure,
    /// and byte-identical regardless of auth mode (ADR-032 duress parity).
    Celebrate {
        haptic: HapticPattern,
        sound: SoundToken,
        animation: AnimationToken,
    },
    /// Atomically replace the contextual controls associated with a surface.
    SetContextBar {
        surface_id: SurfaceId,
        revision: u64,
        bar: Box<ContextBar>,
    },
    /// Present navigation or secondary actions for a surface.
    PresentOverlay {
        surface_id: SurfaceId,
        revision: u64,
        overlay: OverlaySpec,
    },
    /// Apply Core's structural composition for the current window.
    SetPresentationProfile { profile: PresentationProfile },
    /// Atomically replace one named surface with fully prepared,
    /// domain-free presentation primitives.
    ReplaceSurface { surface: SurfaceSpec },
    /// Present a prepared native alert.
    PresentAlert { alert: AlertSpec },
    /// Present transient informational feedback.
    ShowToast { toast: ToastSpec },
    /// Open a prepared URL using the platform default handler.
    OpenExternalUrl { url: String },
    /// Offer prepared bytes through the platform save/share mechanism.
    ExportFile { file: ExportFileSpec },
    /// Perform the platform's default back behavior.
    PerformNativeBack,
    /// Reset platform-owned application state after Core wiped its data.
    ResetApplication,
    /// Post a fully prepared operating-system notification.
    PostNotification { notification: NotificationSpec },
    /// Apply Core's post-biometric authentication decision.
    SetAuthenticationRequirement {
        requirement: AuthenticationRequirement,
    },
}

/// Authentication state the native startup shell must present.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AuthenticationRequirement {
    /// Biometric authentication completed and normal app state may be shown.
    Unlocked,
    /// A configured duress PIN requires the native app-password prompt.
    AppPassword,
}

/// Haptic intent for [`Command::Celebrate`]. Closed enum — platforms map
/// to native patterns (e.g. `UINotificationFeedbackGenerator.success`).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HapticPattern {
    /// The platform's "success" notification haptic.
    Success,
    /// A single light tap.
    Light,
    /// No haptic.
    None,
}

/// Sound intent for [`Command::Celebrate`]. Closed enum — platforms map
/// to a bundled asset; respecting the ringer is the frontend's call.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SoundToken {
    /// The short exchange chime ("clinking glasses").
    ExchangeChime,
    /// No sound.
    None,
}

/// Animation intent for [`Command::Celebrate`]. Closed enum — platforms
/// map to a native one-beat animation, respecting reduce-motion.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AnimationToken {
    /// The two cards meet — the exchange-success beat (~600 ms, then still).
    CardsMeet,
    /// No animation.
    None,
}

/// Screen orientation a frontend should pin to via
/// [`Command::SetOrientationLock`]. Values mirror the platform-native
/// vocabulary (`Activity.requestedOrientation` on Android,
/// `UIInterfaceOrientationMask` on iOS) but are platform-neutral —
/// each frontend maps to its OS API.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Orientation {
    /// Portrait, top of the device pointing up.
    Portrait,
    /// Landscape, with no preference for left or right rotation. Most
    /// platforms map this to "user-rotatable landscape" (the device
    /// can rotate between left and right freely while staying
    /// landscape-locked).
    Landscape,
}

/// Which physical direction a BLE link was established in, reported by the
/// shell on [`Event::BleConnected`]. This is the ground truth for the exchange
/// protocol role: the GATT **central** (it dialed out) drives the handshake as
/// initiator; the GATT **peripheral** (it was connected to) responds. Deriving
/// the role from the physical link — not the pre-connection token tiebreak —
/// is what lets asymmetric BLE discovery self-heal: whichever side actually
/// connects becomes the initiator, even if the token said otherwise
/// (`_private/docs/designs/2026-07-22-role-tiebreak-and-glare-design.md`).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum BleLinkDirection {
    /// This device is the GATT central — it dialed out to the peer's peripheral.
    Outbound,
    /// This device is the GATT peripheral — the peer connected to it.
    Inbound,
}

/// Why a file picker is being opened — lets frontends label the dialog
/// (e.g., "Import Contacts" vs "Import Backup") without hardcoded strings.
///
/// Variants map 1:1 to a label key in the locale store. `Other` covers
/// future imports (e.g., key-bundle import) without forcing every consumer
/// to update for a new well-known purpose.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FilePickPurpose {
    /// vCard / VCF import on top of the contacts engine.
    ImportContacts,
    /// Encrypted vauchi backup blob.
    ImportBackup,
    /// Reserved for future imports — frontends look up `label_key`
    /// in the locale store.
    Other { label_key: String },
}

impl Command {
    /// Returns the variant name without payload data (safe for diagnostics).
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::QrDisplay { .. } => "QrDisplay",
            Self::QrRequestScan => "QrRequestScan",
            Self::ScheduleWakeup { .. } => "ScheduleWakeup",
            Self::BleStartAdvertising { .. } => "BleStartAdvertising",
            Self::BleStartScanning { .. } => "BleStartScanning",
            Self::BleConnect { .. } => "BleConnect",
            Self::BleWriteCharacteristic { .. } => "BleWriteCharacteristic",
            Self::BleReadCharacteristic { .. } => "BleReadCharacteristic",
            Self::BleDisconnect { .. } => "BleDisconnect",
            Self::NfcActivate { .. } => "NfcActivate",
            Self::NfcDeactivate => "NfcDeactivate",
            Self::NfcSendApdu { .. } => "NfcSendApdu",
            Self::AudioEmitChallenge { .. } => "AudioEmitChallenge",
            Self::AudioListenForResponse { .. } => "AudioListenForResponse",
            Self::AudioStop => "AudioStop",
            Self::AccelerometerStart => "AccelerometerStart",
            Self::AccelerometerStop => "AccelerometerStop",
            Self::RelayEscrowDeposit { .. } => "RelayEscrowDeposit",
            Self::RelayEscrowCheck { .. } => "RelayEscrowCheck",
            Self::RelayEscrowRetrieve { .. } => "RelayEscrowRetrieve",
            Self::ShowShareSheet { .. } => "ShowShareSheet",
            Self::BleStopScanning => "BleStopScanning",
            Self::DirectSend { .. } => "DirectSend",
            Self::DirectSendCard { .. } => "DirectSendCard",
            Self::ImagePickFromLibrary => "ImagePickFromLibrary",
            Self::ImageCaptureFromCamera => "ImageCaptureFromCamera",
            Self::ImagePickFromFile => "ImagePickFromFile",
            Self::SwitchCamera { .. } => "SwitchCamera",
            Self::FilePickFromUser { .. } => "FilePickFromUser",
            Self::SetScreenBrightness { .. } => "SetScreenBrightness",
            Self::SetIdleTimerDisabled { .. } => "SetIdleTimerDisabled",
            Self::SetOrientationLock { .. } => "SetOrientationLock",
            Self::LocationRequest { .. } => "LocationRequest",
            Self::Celebrate { .. } => "Celebrate",
            Self::SetContextBar { .. } => "SetContextBar",
            Self::PresentOverlay { .. } => "PresentOverlay",
            Self::SetPresentationProfile { .. } => "SetPresentationProfile",
            Self::ReplaceSurface { .. } => "ReplaceSurface",
            Self::PresentAlert { .. } => "PresentAlert",
            Self::ShowToast { .. } => "ShowToast",
            Self::OpenExternalUrl { .. } => "OpenExternalUrl",
            Self::ExportFile { .. } => "ExportFile",
            Self::PerformNativeBack => "PerformNativeBack",
            Self::ResetApplication => "ResetApplication",
            Self::PostNotification { .. } => "PostNotification",
            Self::SetAuthenticationRequirement { .. } => "SetAuthenticationRequirement",
        }
    }
}

/// A hardware event reported by the frontend back to core.
///
/// These are the results of previously issued [`Command`]s or
/// asynchronous hardware notifications (e.g., BLE discovery, NFC tap).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    /// The user scanned a QR code containing `data`.
    QrScanned { data: String },

    /// Per-frame scan progress from the camera viewfinder.
    ///
    /// Frontends send this periodically (e.g., every 200-500 ms) while the
    /// QR scanner is active. Core uses the rolling detection rate to compute
    /// a [`ScanQuality`] indicator for the viewfinder frame color.
    ///
    /// - `detected`: whether a QR code was found in this frame
    /// - `confidence`: optional platform-specific confidence score (0-100)
    /// - `frame_skipped`: true if the scanner skipped this frame (e.g.,
    ///   sharpness gating). Skipped frames are excluded from the quality
    ///   calculation — they indicate camera settling, not wrong pointing.
    QrScanProgress {
        detected: bool,
        confidence: Option<u8>,
        #[serde(default)]
        frame_skipped: bool,
    },

    /// A BLE peripheral was discovered during scanning.
    BleDeviceDiscovered {
        id: String,
        rssi: i16,
        adv_data: Vec<u8>,
    },
    /// Successfully connected to a BLE device. `direction` reports whether this
    /// device dialed out (GATT central → [`BleLinkDirection::Outbound`]) or was
    /// connected to (GATT peripheral → [`BleLinkDirection::Inbound`]); the
    /// handshake role is derived from it, not from the token tiebreak.
    BleConnected {
        device_id: String,
        direction: BleLinkDirection,
    },
    /// Data read from a BLE characteristic (response to `BleReadCharacteristic`).
    ///
    /// `device_id` names the link the data arrived on so core can reject
    /// events from a stale/losing link under glare instead of feeding them
    /// into the surviving session.
    BleCharacteristicRead {
        device_id: String,
        direction: BleLinkDirection,
        uuid: String,
        data: Vec<u8>,
    },
    /// BLE characteristic notification received (unsolicited push from peripheral).
    /// `device_id` names the link the notification arrived on (see
    /// [`Event::BleCharacteristicRead`]).
    BleCharacteristicNotified {
        device_id: String,
        direction: BleLinkDirection,
        uuid: String,
        data: Vec<u8>,
    },
    /// One BLE link was lost or closed. `device_id` + `direction` identify
    /// exactly which link, so dropping a losing glare link is distinguishable
    /// from losing the surviving session's link.
    BleDisconnected {
        device_id: String,
        direction: BleLinkDirection,
        reason: String,
    },

    /// NFC data received from a tap exchange.
    NfcDataReceived { data: Vec<u8> },

    /// Raw PCM samples from a microphone listen.
    ///
    /// Core decodes the FSK signal internally — the frontend ships
    /// whatever it captured at its native rate. Mono float PCM.
    AudioSamplesRecorded { samples: Vec<f32>, sample_rate: u32 },

    /// A hardware operation failed.
    HardwareError { transport: String, error: String },
    /// The requested hardware is not available on this platform.
    HardwareUnavailable { transport: String },
    /// The user denied the required permission for this hardware.
    ///
    /// Distinct from `HardwareUnavailable` (hardware absent) — the hardware
    /// exists but the OS permission was denied. Frontends should send this
    /// when a runtime permission prompt is rejected (camera, BLE, microphone).
    PermissionDenied { transport: String },

    /// The device's current location, in reply to [`Command::LocationRequest`]
    /// (ADR-051 contact annotations). Coordinates are decimal degrees;
    /// `accuracy_meters` is the provider's reported horizontal accuracy, if any.
    LocationResult {
        latitude: f64,
        longitude: f64,
        accuracy_meters: Option<f32>,
    },

    /// Accelerometer sample from the device.
    ///
    /// Acceleration is reported in milli-g (thousandths of standard gravity)
    /// to avoid `f32` and keep the type `Eq`-compatible across FFI boundaries.
    AccelerometerData {
        timestamp_ms: u64,
        x_milli_g: i32,
        y_milli_g: i32,
        z_milli_g: i32,
    },
    /// Impact detected by the device accelerometer.
    ImpactDetected {
        timestamp_ms: u64,
        magnitude_milli_g: i32,
    },

    /// Relay escrow gate has reached required deposit count.
    RelayEscrowReady { gate_hash: Vec<u8> },
    /// Relay escrow deposit/retrieve failed or gate expired.
    RelayEscrowFailed { gate_hash: Vec<u8>, reason: String },

    /// User shared the link via share sheet.
    LinkShared,
    /// Link was opened by peer, providing their public key.
    LinkOpened { peer_public_key: Vec<u8> },

    // ── Relay escrow (added after v0.13 — append-only to preserve discriminants) ──
    /// Blob retrieved from relay escrow gate (response to `RelayEscrowRetrieve`).
    RelayEscrowBlobReceived { gate_hash: Vec<u8>, blob: Vec<u8> },

    /// Peer's exchange payload received over a direct transport.
    ///
    /// Sent by the frontend after completing the TCP exchange requested
    /// by [`Command::DirectSend`]. Contains the raw bytes of
    /// the peer's exchange payload (QR data string format).
    DirectPayloadReceived { data: Vec<u8> },

    // ── Image picking (avatar editor, ADR-042) ─────────────────────
    // Appended to preserve serde discriminant ordering.
    /// Image data received from photo library, camera, or file picker.
    ///
    /// The frontend sends raw image bytes (PNG, JPEG, etc.) — core
    /// handles format detection and normalization to WebP.
    ImageReceived { data: Vec<u8> },
    /// The user cancelled the image picker / camera without selecting.
    ImagePickCancelled,

    // ── File picking (vCard / backup import, ADR-031) ──────────────
    // Appended to preserve serde discriminant ordering.
    /// File data received from a [`Command::FilePickFromUser`]
    /// request.
    ///
    /// `bytes` is the entire file payload (no decoding — decoding lives
    /// in core). `filename` is the OS-reported display name; some
    /// platforms do not expose it, in which case the frontend reports
    /// an empty string.
    FilePickedFromUser { bytes: Vec<u8>, filename: String },
    /// The user dismissed the file picker without selecting a file.
    FilePickCancelledByUser,

    // ── Biometric auth (ADR-031) ───────────────────────────────────
    // Appended to preserve serde discriminant ordering.
    /// Platform biometric authentication (LAContext on iOS,
    /// BiometricPrompt on Android) succeeded. Core consults its
    /// duress-PIN state to decide the next step; the result rides back
    /// to the frontend as `ActionResult::BiometricUnlockOutcome`.
    BiometricUnlockSucceeded,

    // ── BLE MTU negotiation (slice 32m T2.1) ───────────────────────
    // Appended to preserve serde discriminant ordering.
    /// MTU negotiated for the active BLE connection. `mtu` is the
    /// raw value reported by the GATT stack (Android `MTU_CHANGED`
    /// callback, CoreBluetooth `maximumWriteValueLength + 3`).
    /// Core subtracts the 3-byte ATT header internally before
    /// chunking via `BleChunker::new(data, mtu - 3)`.
    ///
    /// Decoupled from `BleConnected` so a re-negotiation mid-session
    /// (Android `requestMtu` after connect) has a home, and so
    /// platforms whose connection completes before MTU resolves
    /// don't have to delay `BleConnected` waiting for it. Frontends
    /// that don't programmatically observe MTU (desktop Bluetooth
    /// stacks) simply never emit this event — the chunker stays at
    /// `BLE_DEFAULT_USABLE` (20 bytes per ATT minimum).
    ///
    /// Per `_private/docs/designs/2026-05-28-slice-32m-phase-0-event-command-mapping-design.md` §3.2.
    BleMtuNegotiated { device_id: String, mtu: u32 },

    // Appended to preserve serde discriminant ordering.
    /// The peer's AEAD-encrypted `ContactCard`, reported by the frontend after
    /// the second USB / direct-TCP swap requested by [`Command::DirectSendCard`].
    /// The session decrypts it under the HKDF-derived card key and completes the
    /// exchange (USB is physical → proximity High, no user step).
    DirectCardReceived { ciphertext: Vec<u8> },
    /// A Core-minted interaction on a presented surface was activated.
    ActionActivated {
        surface_id: SurfaceId,
        interaction_id: InteractionId,
    },
    /// Available presentation facts changed for the current window.
    PresentationEnvironmentChanged {
        available_width: u32,
        available_height: u32,
        input_modes: Vec<InputMode>,
        motion: MotionPreference,
    },
    /// A visible surface became the target of user interaction.
    SurfaceActivated { surface_id: SurfaceId },
    /// Native or contextual Back was requested for a surface.
    BackRequested { surface_id: SurfaceId },
    /// A presented overlay was dismissed without activating an item.
    OverlayDismissed {
        surface_id: SurfaceId,
        kind: OverlayKind,
    },
    /// A raw value changed for an opaque binding on the current surface.
    ValueChanged {
        surface_id: SurfaceId,
        binding_id: BindingId,
        value: InputValue,
    },
    /// The operating system delivered an opaque application deep link.
    DeepLinkOpened { uri: String },
    /// The application moved to the background or lost its active session.
    AppBackgrounded,
    /// Core reported that background state changed outside an interaction.
    ///
    /// Shells receive only a parameter-free wake signal, then return this
    /// canonical event. Core owns cache invalidation and emits the complete
    /// replacement command batch; no screen identifiers cross the boundary.
    PresentationInvalidated,
}

// INLINE_TEST_REQUIRED: serde roundtrip tests need private enum variant access
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn qr_display_command_stores_data() {
        let cmd = Command::QrDisplay {
            data: "vauchi://exchange/abc123".into(),
        };
        assert!(matches!(cmd, Command::QrDisplay { data } if data == "vauchi://exchange/abc123"));
    }

    // @internal
    #[test]
    fn ble_start_advertising_stores_payload() {
        let payload = vec![0x01, 0x02, 0x03];
        let cmd = Command::BleStartAdvertising {
            service_uuid: "12345678-1234-1234-1234-123456789abc".into(),
            payload: payload.clone(),
        };
        assert!(
            matches!(cmd, Command::BleStartAdvertising { service_uuid, payload: p }
                if service_uuid == "12345678-1234-1234-1234-123456789abc" && p == payload)
        );
    }

    // @internal
    #[test]
    fn audio_listen_stores_timeout() {
        let cmd = Command::AudioListenForResponse {
            timeout_ms: 5000,
            sample_rate: 44100,
        };
        assert!(matches!(
            cmd,
            Command::AudioListenForResponse {
                timeout_ms,
                sample_rate,
            } if timeout_ms == 5000 && sample_rate == 44100
        ));
    }

    // @internal
    #[test]
    fn qr_scanned_event_stores_data() {
        let evt = Event::QrScanned {
            data: "scanned-data".into(),
        };
        assert!(matches!(evt, Event::QrScanned { data } if data == "scanned-data"));
    }

    // @internal
    #[test]
    fn ble_device_discovered_stores_rssi() {
        let evt = Event::BleDeviceDiscovered {
            id: "device-1".into(),
            rssi: -42,
            adv_data: vec![0xFF],
        };
        assert!(matches!(evt, Event::BleDeviceDiscovered { rssi, .. } if rssi == -42));
    }

    // @internal
    #[test]
    fn hardware_unavailable_stores_transport() {
        let evt = Event::HardwareUnavailable {
            transport: "BLE".into(),
        };
        assert!(matches!(evt, Event::HardwareUnavailable { transport } if transport == "BLE"));
    }

    // @internal
    #[test]
    fn permission_denied_stores_transport() {
        let evt = Event::PermissionDenied {
            transport: "camera".into(),
        };
        assert!(matches!(evt, Event::PermissionDenied { transport } if transport == "camera"));
    }

    // @internal
    #[test]
    fn permission_denied_is_distinct_from_hardware_unavailable() {
        let denied = Event::PermissionDenied {
            transport: "camera".into(),
        };
        let unavailable = Event::HardwareUnavailable {
            transport: "camera".into(),
        };
        assert_ne!(denied, unavailable);
    }

    // @internal
    #[test]
    fn hardware_error_stores_details() {
        let evt = Event::HardwareError {
            transport: "NFC".into(),
            error: "no reader detected".into(),
        };
        assert!(matches!(evt, Event::HardwareError { transport, error }
                if transport == "NFC" && error == "no reader detected"));
    }

    // @internal
    #[test]
    fn command_serialization_roundtrip() {
        let commands = vec![
            Command::QrDisplay {
                data: "test".into(),
            },
            Command::QrRequestScan,
            Command::BleStartAdvertising {
                service_uuid: "uuid".into(),
                payload: vec![1, 2, 3],
            },
            Command::BleDisconnect {
                device_id: "d1".into(),
                direction: BleLinkDirection::Outbound,
            },
            Command::NfcActivate {
                payload: vec![0xAA],
            },
            Command::NfcDeactivate,
            Command::AudioEmitChallenge {
                samples: vec![0.1, 0.2, 0.3],
                sample_rate: 44100,
            },
            Command::AudioListenForResponse {
                timeout_ms: 3000,
                sample_rate: 44100,
            },
            Command::AudioStop,
            Command::AccelerometerStart,
            Command::AccelerometerStop,
            Command::RelayEscrowDeposit {
                gate_hash: vec![0xAB; 32],
                slot_hash: vec![0xCD; 32],
                encrypted_card: vec![0x01; 64],
                ttl_seconds: 3600,
            },
            Command::RelayEscrowCheck {
                gate_hash: vec![0xAB; 32],
                suggested_interval_ms: 30_000,
            },
            Command::RelayEscrowRetrieve {
                gate_hash: vec![0xAB; 32],
                slot_hash: vec![0xCD; 32],
            },
            Command::ShowShareSheet {
                url: "https://vauchi.app/link/abc123".into(),
            },
            Command::ImagePickFromLibrary,
            Command::ImageCaptureFromCamera,
            Command::ImagePickFromFile,
            Command::SwitchCamera { use_front: true },
            Command::SwitchCamera { use_front: false },
            Command::FilePickFromUser {
                accepted_mime_types: vec!["text/vcard".into(), "text/x-vcard".into()],
                purpose: FilePickPurpose::ImportContacts,
            },
            Command::FilePickFromUser {
                accepted_mime_types: vec!["application/octet-stream".into()],
                purpose: FilePickPurpose::ImportBackup,
            },
            Command::FilePickFromUser {
                accepted_mime_types: vec![],
                purpose: FilePickPurpose::Other {
                    label_key: "import.key_bundle".into(),
                },
            },
        ];
        for cmd in &commands {
            let json = serde_json::to_string(cmd).expect("serialize");
            let decoded: Command = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(cmd, &decoded, "roundtrip failed for {:?}", cmd);
        }
    }

    // @internal
    #[test]
    fn event_serialization_roundtrip() {
        let events = vec![
            Event::QrScanned { data: "qr".into() },
            Event::BleDeviceDiscovered {
                id: "d1".into(),
                rssi: -60,
                adv_data: vec![],
            },
            Event::BleConnected {
                device_id: "d1".into(),
                // TODO(f0-direction): verify — role-neutral roundtrip test.
                direction: BleLinkDirection::Outbound,
            },
            Event::BleCharacteristicRead {
                device_id: "d1".into(),
                direction: BleLinkDirection::Outbound,
                uuid: "char1".into(),
                data: vec![0x0B],
            },
            Event::BleDisconnected {
                device_id: "d1".into(),
                direction: BleLinkDirection::Outbound,
                reason: "timeout".into(),
            },
            Event::NfcDataReceived { data: vec![0xCC] },
            Event::AudioSamplesRecorded {
                samples: vec![0.0, 0.5, -0.5],
                sample_rate: 44100,
            },
            Event::HardwareError {
                transport: "BLE".into(),
                error: "adapter off".into(),
            },
            Event::HardwareUnavailable {
                transport: "NFC".into(),
            },
            Event::PermissionDenied {
                transport: "camera".into(),
            },
            Event::AccelerometerData {
                timestamp_ms: 1_000,
                x_milli_g: 1_000, // ~1 g lateral
                y_milli_g: 0,
                z_milli_g: -9_800, // ~-9.8 g (gravity)
            },
            Event::ImpactDetected {
                timestamp_ms: 2_000,
                magnitude_milli_g: 3_500, // ~3.5 g impact
            },
            Event::RelayEscrowReady {
                gate_hash: vec![0xDE; 32],
            },
            Event::RelayEscrowFailed {
                gate_hash: vec![0xDE; 32],
                reason: "gate expired".into(),
            },
            Event::LinkShared,
            Event::LinkOpened {
                peer_public_key: vec![0x04; 32],
            },
            Event::ImageReceived {
                data: vec![0xFF, 0xD8, 0xFF],
            },
            Event::ImagePickCancelled,
            Event::QrScanProgress {
                detected: true,
                confidence: Some(85),
                frame_skipped: false,
            },
            Event::QrScanProgress {
                detected: false,
                confidence: None,
                frame_skipped: true,
            },
            Event::FilePickedFromUser {
                bytes: vec![0x42; 8],
                filename: "contacts.vcf".into(),
            },
            Event::FilePickedFromUser {
                bytes: vec![],
                filename: String::new(),
            },
            Event::FilePickCancelledByUser,
            Event::BleMtuNegotiated {
                device_id: "AA:BB:CC:DD:EE:FF".into(),
                mtu: 247,
            },
            Event::BleMtuNegotiated {
                device_id: String::new(),
                mtu: 23, // ATT minimum
            },
        ];
        for evt in &events {
            let json = serde_json::to_string(evt).expect("serialize");
            let decoded: Event = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(evt, &decoded, "roundtrip failed for {:?}", evt);
        }
    }

    // @internal
    #[test]
    fn command_clone_equals_original() {
        let cmd = Command::BleWriteCharacteristic {
            device_id: "d1".into(),
            direction: BleLinkDirection::Outbound,
            uuid: "test-uuid".into(),
            data: vec![1, 2, 3, 4, 5],
        };
        let cloned = cmd.clone();
        assert_eq!(cmd, cloned);
    }

    // @internal
    #[test]
    fn event_clone_equals_original() {
        let evt = Event::BleCharacteristicNotified {
            device_id: "d1".into(),
            direction: BleLinkDirection::Outbound,
            uuid: "notify-uuid".into(),
            data: vec![0xDE, 0xAD],
        };
        let cloned = evt.clone();
        assert_eq!(evt, cloned);
    }

    // @internal
    #[test]
    fn all_command_variants_are_distinct() {
        let variants: Vec<Command> = vec![
            Command::QrDisplay { data: "".into() },
            Command::QrRequestScan,
            Command::BleStartAdvertising {
                service_uuid: "".into(),
                payload: vec![],
            },
            Command::BleStartScanning {
                service_uuid: "".into(),
            },
            Command::BleConnect {
                device_id: "".into(),
            },
            Command::BleWriteCharacteristic {
                device_id: "".into(),
                direction: BleLinkDirection::Outbound,
                uuid: "".into(),
                data: vec![],
            },
            Command::BleReadCharacteristic {
                device_id: "".into(),
                direction: BleLinkDirection::Outbound,
                uuid: "".into(),
            },
            Command::BleDisconnect {
                device_id: "".into(),
                direction: BleLinkDirection::Outbound,
            },
            Command::NfcActivate { payload: vec![] },
            Command::NfcDeactivate,
            Command::AudioEmitChallenge {
                samples: vec![],
                sample_rate: 0,
            },
            Command::AudioListenForResponse {
                timeout_ms: 0,
                sample_rate: 0,
            },
            Command::AudioStop,
            Command::AccelerometerStart,
            Command::AccelerometerStop,
            Command::RelayEscrowDeposit {
                gate_hash: vec![],
                slot_hash: vec![],
                encrypted_card: vec![],
                ttl_seconds: 0,
            },
            Command::RelayEscrowCheck {
                gate_hash: vec![],
                suggested_interval_ms: 0,
            },
            Command::RelayEscrowRetrieve {
                gate_hash: vec![],
                slot_hash: vec![],
            },
            Command::ShowShareSheet { url: "".into() },
            Command::BleStopScanning,
            Command::DirectSend {
                payload: vec![],
                is_initiator: false,
            },
            Command::ImagePickFromLibrary,
            Command::ImageCaptureFromCamera,
            Command::ImagePickFromFile,
            Command::SwitchCamera { use_front: false },
            Command::FilePickFromUser {
                accepted_mime_types: vec![],
                purpose: FilePickPurpose::ImportContacts,
            },
        ];
        // 26 total command variants
        assert_eq!(variants.len(), 26);
    }

    // @internal
    #[test]
    fn all_event_variants_are_distinct() {
        let variants: Vec<Event> = vec![
            Event::QrScanned { data: "".into() },
            Event::BleDeviceDiscovered {
                id: "".into(),
                rssi: 0,
                adv_data: vec![],
            },
            Event::BleConnected {
                device_id: "".into(),
                // TODO(f0-direction): verify — role-neutral distinctness test.
                direction: BleLinkDirection::Outbound,
            },
            Event::BleCharacteristicRead {
                device_id: "".into(),
                direction: BleLinkDirection::Outbound,
                uuid: "".into(),
                data: vec![],
            },
            Event::BleCharacteristicNotified {
                device_id: "".into(),
                direction: BleLinkDirection::Outbound,
                uuid: "".into(),
                data: vec![],
            },
            Event::BleDisconnected {
                device_id: "".into(),
                direction: BleLinkDirection::Outbound,
                reason: "".into(),
            },
            Event::NfcDataReceived { data: vec![] },
            Event::AudioSamplesRecorded {
                samples: vec![],
                sample_rate: 0,
            },
            Event::HardwareError {
                transport: "".into(),
                error: "".into(),
            },
            Event::HardwareUnavailable {
                transport: "".into(),
            },
            Event::PermissionDenied {
                transport: "".into(),
            },
            Event::AccelerometerData {
                timestamp_ms: 0,
                x_milli_g: 0,
                y_milli_g: 0,
                z_milli_g: 0,
            },
            Event::ImpactDetected {
                timestamp_ms: 0,
                magnitude_milli_g: 0,
            },
            Event::RelayEscrowReady { gate_hash: vec![] },
            Event::RelayEscrowFailed {
                gate_hash: vec![],
                reason: "".into(),
            },
            Event::LinkShared,
            Event::LinkOpened {
                peer_public_key: vec![],
            },
            Event::RelayEscrowBlobReceived {
                gate_hash: vec![],
                blob: vec![],
            },
            Event::DirectPayloadReceived { data: vec![] },
            Event::ImageReceived { data: vec![] },
            Event::ImagePickCancelled,
            Event::QrScanProgress {
                detected: false,
                confidence: None,
                frame_skipped: false,
            },
            Event::FilePickedFromUser {
                bytes: vec![],
                filename: String::new(),
            },
            Event::FilePickCancelledByUser,
            Event::BleMtuNegotiated {
                device_id: "AA:BB".into(),
                mtu: 247,
            },
        ];
        // 25 total event variants (+1 BleMtuNegotiated, slice 32m T2.1)
        assert_eq!(variants.len(), 25);
    }

    // ── File-picker variants (Phase 1: types only) ──────────────────

    // @internal
    #[test]
    fn file_pick_from_user_command_stores_purpose_and_mime() {
        let cmd = Command::FilePickFromUser {
            accepted_mime_types: vec!["text/vcard".into(), "text/x-vcard".into()],
            purpose: FilePickPurpose::ImportContacts,
        };
        match cmd {
            Command::FilePickFromUser {
                accepted_mime_types,
                purpose,
            } => {
                assert_eq!(accepted_mime_types, vec!["text/vcard", "text/x-vcard"]);
                assert_eq!(purpose, FilePickPurpose::ImportContacts);
            }
            other => panic!("expected FilePickFromUser, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn file_pick_purpose_other_carries_label_key() {
        let purpose = FilePickPurpose::Other {
            label_key: "import.key_bundle".into(),
        };
        match purpose {
            FilePickPurpose::Other { label_key } => assert_eq!(label_key, "import.key_bundle"),
            other => panic!("expected Other, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn file_pick_purpose_variants_are_distinct() {
        let a = FilePickPurpose::ImportContacts;
        let b = FilePickPurpose::ImportBackup;
        let c = FilePickPurpose::Other {
            label_key: "x".into(),
        };
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
    }

    // @internal
    #[test]
    fn file_pick_purpose_serialization_roundtrip() {
        let purposes = vec![
            FilePickPurpose::ImportContacts,
            FilePickPurpose::ImportBackup,
            FilePickPurpose::Other {
                label_key: "import.key_bundle".into(),
            },
            FilePickPurpose::Other {
                label_key: String::new(),
            },
        ];
        for p in &purposes {
            let json = serde_json::to_string(p).expect("serialize");
            let decoded: FilePickPurpose = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(p, &decoded, "roundtrip failed for {:?}", p);
        }
    }

    // @internal
    #[test]
    fn file_picked_from_user_event_stores_bytes_and_filename() {
        let evt = Event::FilePickedFromUser {
            bytes: vec![0x42, 0x43, 0x44],
            filename: "contacts.vcf".into(),
        };
        match evt {
            Event::FilePickedFromUser { bytes, filename } => {
                assert_eq!(bytes, vec![0x42, 0x43, 0x44]);
                assert_eq!(filename, "contacts.vcf");
            }
            other => panic!("expected FilePickedFromUser, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn file_picked_event_distinct_from_cancellation() {
        let picked = Event::FilePickedFromUser {
            bytes: vec![],
            filename: String::new(),
        };
        let cancelled = Event::FilePickCancelledByUser;
        assert_ne!(picked, cancelled);
    }

    // @internal
    #[test]
    fn file_pick_from_user_variant_name_is_stable() {
        let cmd = Command::FilePickFromUser {
            accepted_mime_types: vec![],
            purpose: FilePickPurpose::ImportBackup,
        };
        assert_eq!(cmd.variant_name(), "FilePickFromUser");
    }

    // @internal
    #[test]
    fn set_screen_brightness_with_some_level_round_trips() {
        let cmd = Command::SetScreenBrightness { level: Some(0.65) };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(
            json.contains("\"SetScreenBrightness\""),
            "expected variant tag in serialized form, got {json}"
        );
        let restored: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, restored);
    }

    // @internal
    #[test]
    fn set_screen_brightness_with_none_means_restore_default() {
        // The contract: `level: None` is the explicit "restore platform
        // default" signal. Keep the wire shape pinned so frontends can
        // distinguish it from a missing field.
        let cmd = Command::SetScreenBrightness { level: None };
        let json = serde_json::to_string(&cmd).unwrap();
        let restored: Command = serde_json::from_str(&json).unwrap();
        match restored {
            Command::SetScreenBrightness { level } => assert_eq!(level, None),
            other => panic!("expected SetScreenBrightness, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn set_screen_brightness_variant_name_is_stable() {
        let cmd = Command::SetScreenBrightness { level: Some(0.5) };
        assert_eq!(cmd.variant_name(), "SetScreenBrightness");
    }

    // @internal
    #[test]
    fn set_idle_timer_disabled_round_trips_each_state() {
        for disabled in [true, false] {
            let cmd = Command::SetIdleTimerDisabled { disabled };
            let json = serde_json::to_string(&cmd).unwrap();
            assert!(
                json.contains("\"SetIdleTimerDisabled\""),
                "expected variant tag, got {json}"
            );
            let restored: Command = serde_json::from_str(&json).unwrap();
            assert_eq!(cmd, restored);
        }
    }

    // @internal
    #[test]
    fn set_idle_timer_disabled_variant_name_is_stable() {
        let cmd = Command::SetIdleTimerDisabled { disabled: true };
        assert_eq!(cmd.variant_name(), "SetIdleTimerDisabled");
    }

    // @internal
    #[test]
    fn screen_presentation_commands_are_distinct() {
        // Sanity-check that the two new variants are not accidentally
        // matched as the same shape (both carry an option-like field
        // that could collide if someone writes a sloppy match).
        let bright = Command::SetScreenBrightness { level: None };
        let idle = Command::SetIdleTimerDisabled { disabled: false };
        assert_ne!(bright, idle);
        assert_ne!(bright.variant_name(), idle.variant_name());
    }
}
