// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-stage face-to-face exchange engine.
//!
//! Pair 4 of `_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens`.
//! Renders the simultaneous bilateral QR + camera flow that
//! [`vauchi_core::exchange::MultiStageSession`] drives. The native
//! screens previously consumed `MobileMultiStageSession` directly via
//! cycle-thread listener callbacks and produced no `ScreenModel` —
//! this engine bridges that push model into the pull/event model
//! `CoreScreenView` expects.
//!
//! # Bridge contract
//!
//! `MultiStageSession` runs on a dedicated `vauchi-exchange-cycle`
//! thread. It pushes state via `MultiStageSessionListener` callbacks
//! that fire from the cycle thread. The bridge sits at the AppEngine
//! layer (Phase 4a A5 wiring): listener callbacks are translated into
//! `MultiStageExchangeEngine::set_state` /
//! `MultiStageExchangeEngine::set_qr_payload` /
//! `MultiStageExchangeEngine::set_finalized` calls on the AppEngine's
//! cached engine instance, then `UpdateScreen(current_screen())` is
//! emitted. Frontends marshal the listener callback to their UI
//! thread before calling into AppEngine.
//!
//! Hardware events flow through `handle_hardware_event` per ADR-031:
//! `QrScanProgress` updates the [`ScanQualityTracker`],
//! `HardwareUnavailable { transport: "camera" }` and
//! `PermissionDenied { transport: "camera" }` flip the engine into
//! the corresponding chrome.
//!
//! # Animated-QR frame carrier (ADR-044 Amendment 2a, C2a — data half)
//!
//! The own-QR ships its frame(s) in `QrCode.frames` so animated QR is
//! render *data*, not a frontend behavior. The cycle thread pushes one
//! live frame at a time via `set_qr_payload` (each embeds the current ACK
//! bitmap from the stateful `MultiStageSession::get_display_qr`), so
//! `frames` is that single frame today; the deferred half plumbs the
//! session's full chunk snapshot as a list. `data` stays set for the
//! pre-migration path.

use vauchi_core::Event;
use vauchi_core::exchange::{
    AccelerometerProximityState, AudioProximityState, ProtocolState, QrPayload,
};

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::exchange::scan_quality::ScanQualityTracker;
use crate::ui::*;

#[path = "multi_stage_exchange_camera_gate.rs"]
mod camera_gate;
use camera_gate::CameraGate;

// ── Action IDs ─────────────────────────────────────────────────────

/// User dismissed / went back from the exchange screen.
pub const CANCEL_ACTION_ID: &str = "cancel";
/// User tapped the retry button after a `Failed` state.
pub const RETRY_ACTION_ID: &str = "retry";
/// User tapped the done button on the success terminal screen.
pub const DONE_ACTION_ID: &str = "done";
/// User toggled front/rear camera.
pub const SWITCH_CAMERA_ACTION_ID: &str = "switch_camera";
/// User tapped the "grant permission" affordance after camera permission
/// was denied. Frontend re-prompts the OS permission dialog.
pub const GRANT_CAMERA_PERMISSION_ACTION_ID: &str = "grant_camera_permission";

// ── Component IDs ──────────────────────────────────────────────────

const COMPONENT_ID_OWN_QR: &str = "own_qr";
/// Component id of the peer-scanning `QrCode { mode: Scan }` rendered
/// while the multi-stage exchange is active. Frontends emit
/// `UserAction::TextChanged { component_id: PEER_SCAN_COMPONENT_ID, value }`
/// per the existing `exchange_qr.rs` single-direction contract; the
/// platform layer auto-routes those scans into the live cycle-thread
/// session (see `core/vauchi-platform/src/platform_app_engine.rs`).
pub const PEER_SCAN_COMPONENT_ID: &str = "peer_scan";
const COMPONENT_ID_PEER_SCAN: &str = PEER_SCAN_COMPONENT_ID;
/// Component id of the `ActionList` that holds the switch-camera /
/// cancel buttons. It lives inside the active screen's preview `Row`
/// (so the buttons sit beside the camera preview); its taps arrive as
/// `UserAction::ListItemSelected` and are normalised back to action ids
/// in `handle_action`.
const EXCHANGE_ACTIONS_ID: &str = "exchange_actions";
/// Component id of the `Row` grouping the peer-scan preview with the
/// action buttons on the active exchange screen.
const EXCHANGE_PREVIEW_ROW_ID: &str = "exchange_preview_row";
const COMPONENT_ID_STATUS: &str = "status";
const COMPONENT_ID_PEER_NAME: &str = "peer_name";
const COMPONENT_ID_PERMISSION: &str = "permission_required";
const COMPONENT_ID_HARDWARE: &str = "hardware_unavailable";

// ── Screen IDs ─────────────────────────────────────────────────────

/// The single screen id this engine owns. Sub-screens (success
/// overlay, failure overlay, permission gate) all share the id —
/// frontends differentiate by inspecting the components list.
pub const SCREEN_ID: &str = "multi_stage_exchange";

/// Engine for the multi-stage face-to-face exchange screen.
///
/// Pure state container — does not own the
/// [`MobileMultiStageSession`](vauchi_platform::multistage_exchange::MobileMultiStageSession).
/// The AppEngine bridge wires listener callbacks into `set_state`/
/// `set_qr_payload`/`set_finalized` (Phase 4a A5).
#[derive(Clone, Debug)]
pub struct MultiStageExchangeEngine {
    state: ProtocolState,
    /// Latest QR data emitted by the cycle thread for the local card —
    /// rendered as a `QrCode { mode: Display }` to the peer.
    current_qr_data: Option<String>,
    /// Peer display name — set on the Finalized transition.
    peer_name: Option<String>,
    /// Rich success-screen summary (received card + group + visibility),
    /// assembled by the AppEngine at finalize. `None` falls back to the
    /// minimal "Exchange Complete" chrome
    /// (`2026-06-04-exchange-terminal-screens`).
    success_summary: Option<crate::ui::exchange::success::ExchangeSuccessSummary>,
    /// Cycle thread reported `on_session_ended` — terminal cleanup
    /// done, frontend may dismiss the screen.
    session_ended: bool,
    /// Camera availability gate. Drives the permission/hardware
    /// fallback chrome.
    camera_gate: CameraGate,
    /// Front-camera toggle. Surfaces in the `SwitchCamera` command.
    use_front_camera: bool,
    /// Rolling viewfinder quality indicator.
    scan_quality_tracker: ScanQualityTracker,
    /// `true` once `cancel` was pressed — disables further state
    /// pushes so a late callback cannot un-cancel the screen.
    cancelled: bool,
    /// Audio-proximity verification state — Hover-only. Default
    /// `Pending` for both Glance and Hover; Glance never transitions
    /// because it doesn't emit the audio commands.
    audio_proximity: AudioProximityState,
    /// TapHoverShake accelerometer-proximity mirror. Pending for Glance
    /// and Hover (never driven); TapHoverShake transitions it via
    /// [`Self::set_accel_proximity`]. See [`AccelerometerProximityState`].
    accel_proximity: AccelerometerProximityState,
    /// Mode marker — `true` for engines constructed via
    /// [`Self::new_hover`], `false` for [`Self::new_glance`].
    /// Consumed by the platform-binding wire-up to decide whether
    /// to register the audio-proximity listener on the cycle-thread
    /// session: without that listener, the autonomous audio trigger
    /// stays a silent no-op, so a Glance flow never surfaces the
    /// audio chrome (Phase 1.C polish — keeps the spurious trigger
    /// in `try_autonomous_audio_trigger` from advancing the inner
    /// state machine for non-Hover sessions even before the Phase
    /// 1.E mode-dispatcher flips to per-mode constructors).
    is_hover_mode: bool,
    locale: Locale,
}

impl MultiStageExchangeEngine {
    /// Construct an engine wired for the Glance flow (bilateral QR
    /// scan, no audio proximity). Replaces the prior `new()` body
    /// verbatim; the renamed entry point preserves call-site
    /// behaviour while making room for mode-aware variants
    /// (`new_hover` etc.) in subsequent Phase 1 sub-steps.
    pub fn new_glance() -> Self {
        Self {
            state: ProtocolState::Idle,
            current_qr_data: None,
            peer_name: None,
            success_summary: None,
            session_ended: false,
            camera_gate: CameraGate::Available,
            use_front_camera: false,
            scan_quality_tracker: ScanQualityTracker::new(),
            cancelled: false,
            audio_proximity: AudioProximityState::Pending,
            accel_proximity: AccelerometerProximityState::Pending,
            is_hover_mode: false,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-10).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Construct an engine wired for the Hover flow (bilateral QR
    /// scan + ultrasonic audio-proximity verification). Differs
    /// from [`Self::new_glance`] in two ways: the front camera is
    /// the default (face-to-face screen-to-screen UX), and
    /// `audio_proximity` starts at `Pending` ready for the chirp
    /// handshake the engine triggers when it reaches the active
    /// scanning state.
    ///
    /// Phase 1 of the legacy `ExchangeMode::Hover` graduation — the
    /// dispatcher in `core/vauchi-app/src/ui/exchange.rs` flips to
    /// call this constructor in Phase 1.E (mode-dispatch step).
    pub fn new_hover() -> Self {
        Self {
            state: ProtocolState::Idle,
            current_qr_data: None,
            peer_name: None,
            success_summary: None,
            session_ended: false,
            camera_gate: CameraGate::Available,
            use_front_camera: true,
            scan_quality_tracker: ScanQualityTracker::new(),
            cancelled: false,
            audio_proximity: AudioProximityState::Pending,
            accel_proximity: AccelerometerProximityState::Pending,
            is_hover_mode: true,
            locale: Locale::English,
        }
    }

    /// Construct an engine wired for the TapHoverShake flow — front
    /// camera + ultrasonic audio proximity (like Hover) plus the
    /// accelerometer shake signal. `is_hover_mode` is `true` because the
    /// audio handshake runs; `accel_proximity` starts `Pending`. The
    /// accel cross-correlation chrome lands with the envelope-transport
    /// follow-up (P2.C).
    pub fn new_tap_hover_shake() -> Self {
        Self {
            state: ProtocolState::Idle,
            current_qr_data: None,
            peer_name: None,
            success_summary: None,
            session_ended: false,
            camera_gate: CameraGate::Available,
            use_front_camera: true,
            scan_quality_tracker: ScanQualityTracker::new(),
            cancelled: false,
            audio_proximity: AudioProximityState::Pending,
            accel_proximity: AccelerometerProximityState::Pending,
            is_hover_mode: true,
            locale: Locale::English,
        }
    }

    // ── Bridge setters (called by AppEngine from listener callbacks) ─

    /// Update the protocol state. Called by the AppEngine bridge after
    /// `MultiStageSessionListener::on_state_changed` fires on the
    /// cycle thread. No-op while cancelled.
    pub fn set_state(&mut self, state: ProtocolState) {
        if self.cancelled {
            return;
        }
        self.state = state;
    }

    /// Update the current QR payload. Called by the AppEngine bridge
    /// after `MultiStageSessionListener::on_qr_payload`. No-op while
    /// cancelled.
    pub fn set_qr_payload(&mut self, payload: &QrPayload) {
        if self.cancelled {
            return;
        }
        self.current_qr_data = Some(payload.data.clone());
    }

    /// Update the audio-proximity state. Called by the AppEngine
    /// bridge after the session-side audio handshake reports a
    /// transition (Phase 1.C.3 wires the listener under Option B
    /// from `2026-04-28-multi-stage-engine-hover-ultrasonic`'s
    /// `investigation.md`). No-op while cancelled.
    ///
    /// Glance never invokes this — `audio_proximity` stays
    /// `Pending` for the lifetime of a Glance exchange so the
    /// status indicator and `build_screen` Failed branch remain
    /// unchanged for that mode. Hover transitions through
    /// `Pending → Listening → Confirmed` on success or
    /// `Pending → Listening → Failed` on the proximity timeout.
    pub fn set_audio_proximity(&mut self, state: AudioProximityState) {
        if self.cancelled {
            return;
        }
        self.audio_proximity = state;
    }

    /// Mark the exchange finalized with the peer's display name.
    /// Called after `MultiStageSessionListener::on_finalized`. The
    /// `Finalized` state will already have been pushed via
    /// `set_state`; this only attaches the name for the success
    /// screen.
    pub fn set_finalized(&mut self, peer_name: String) {
        if self.cancelled {
            return;
        }
        self.peer_name = Some(peer_name);
    }

    /// Attach the rich success-screen summary (received card + group +
    /// visibility) the AppEngine assembles at finalize. No-op while
    /// cancelled (2026-06-04-exchange-terminal-screens).
    pub fn set_success_summary(
        &mut self,
        summary: crate::ui::exchange::success::ExchangeSuccessSummary,
    ) {
        if self.cancelled {
            return;
        }
        self.success_summary = Some(summary);
    }

    /// Mark the cycle thread as ended. Called after
    /// `MultiStageSessionListener::on_session_ended`. Flips the
    /// success/failure screens from "keep pointing…" to a terminal
    /// affordance row.
    pub fn set_session_ended(&mut self) {
        self.session_ended = true;
    }

    /// Currently-selected camera (front == true).
    pub fn use_front_camera(&self) -> bool {
        self.use_front_camera
    }

    /// Returns `true` for engines constructed via
    /// [`Self::new_hover`], `false` for [`Self::new_glance`].
    /// The platform-binding layer reads this to decide whether to
    /// register the cycle-thread audio listener — without that
    /// listener registration, the autonomous audio trigger in
    /// `vauchi-platform::multistage_exchange::try_autonomous_audio_trigger`
    /// short-circuits before advancing the session state machine,
    /// so Glance flows never surface the audio chrome.
    pub fn is_hover_mode(&self) -> bool {
        self.is_hover_mode
    }

    /// Current audio-proximity state — `Pending` for Glance throughout
    /// the exchange (the mode never transitions the field). For Hover,
    /// transitions through `Listening → Confirmed` on success or
    /// → `Failed` on the proximity timeout.
    pub fn audio_proximity(&self) -> AudioProximityState {
        self.audio_proximity
    }

    /// Drive the engine's accelerometer-proximity mirror. TapHoverShake
    /// only — Glance and Hover never call this, so `accel_proximity`
    /// stays `Pending` and the status detail + Failed branch are
    /// unchanged for those modes. No-op after cancel, mirroring
    /// [`Self::set_audio_proximity`].
    pub fn set_accel_proximity(&mut self, state: AccelerometerProximityState) {
        if self.cancelled {
            return;
        }
        self.accel_proximity = state;
    }

    /// Returns the engine's accelerometer-proximity mirror. `Pending`
    /// for Glance and Hover; TapHoverShake drives it through the shake
    /// states (`Listening → Confirmed` on cross-correlation success or
    /// `→ Failed` on timeout/mismatch).
    pub fn accel_proximity(&self) -> AccelerometerProximityState {
        self.accel_proximity
    }

    // ── Internal helpers ───────────────────────────────────────────

    fn build_screen(&self) -> ScreenModel {
        let title = self.t("exchange.title");

        // Permission/hardware gate trumps protocol-state chrome — the
        // user cannot make progress without the camera.
        if matches!(self.camera_gate, CameraGate::PermissionDenied) {
            return ScreenModel::new(
                SCREEN_ID,
                title,
                vec![Component::StatusIndicator {
                    id: COMPONENT_ID_PERMISSION.into(),
                    icon: Some("camera.slash".into()),
                    title: self.t("multi_stage.camera_required_title"),
                    detail: Some(self.t("multi_stage.camera_required_detail")),
                    status: Status::Warning,
                    status_label: self.t(Status::Warning.label_key()),
                    a11y: None,
                }],
                vec![
                    ScreenAction {
                        id: GRANT_CAMERA_PERMISSION_ACTION_ID.into(),
                        label: self.t("exchange.grant_permission"),
                        style: ActionStyle::Primary,
                        enabled: true,
                        a11y: Some(A11y::labeled(self.t("exchange.grant_permission"))),
                    },
                    ScreenAction {
                        id: CANCEL_ACTION_ID.into(),
                        label: self.t("action.cancel"),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: Some(A11y::labeled(self.t("action.cancel"))),
                    },
                ],
            );
        }
        if matches!(self.camera_gate, CameraGate::Unavailable) {
            return ScreenModel::new(
                SCREEN_ID,
                title,
                vec![Component::StatusIndicator {
                    id: COMPONENT_ID_HARDWARE.into(),
                    icon: Some("camera.slash".into()),
                    title: self.t("multi_stage.camera_unavailable_title"),
                    detail: Some(self.t("multi_stage.camera_unavailable_detail")),
                    status: Status::Failed,
                    status_label: self.t(Status::Failed.label_key()),
                    a11y: None,
                }],
                vec![ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                }],
            );
        }

        // Audio-proximity failure takes precedence over protocol-state
        // narration because it surfaces a *user-facing physical-setup*
        // problem ("devices aren't actually close") that a generic
        // "Exchange Failed" panel would hide. G1.3 of the Hover
        // graduation problem record. Glance never reaches this branch
        // because Glance's audio_proximity stays Pending forever.
        if matches!(self.audio_proximity, AudioProximityState::Failed) {
            return self.build_audio_failed_screen(title);
        }
        // Accel-proximity failure is the TapHoverShake mirror, checked
        // after audio so audio wins the single Failed screen when both
        // signals fail (see `audio_failed_takes_precedence_over_accel_failed`).
        if matches!(self.accel_proximity, AccelerometerProximityState::Failed) {
            return self.build_accel_failed_screen(title);
        }
        match &self.state {
            ProtocolState::Failed(reason) => self.build_failed_screen(title, reason),
            ProtocolState::Finalized | ProtocolState::Complete | ProtocolState::RetryReady
                if self.session_ended =>
            {
                self.build_success_screen(title)
            }
            // Finalized before the grace expires: the contact IS already
            // persisted (persist fires on the Finalized event), and the
            // FINALIZED_GRACE broadcast exists only for the peer — a
            // still-Complete peer needs to scan our RDYY (two-generals
            // last-ack, session.rs FINALIZED_GRACE_DURATION). So show
            // Success now with the QR still broadcasting instead of
            // parking the user on "Almost done" for the whole window
            // (2026-07-01-hover-exchange-completion-latency). Complete/
            // RetryReady keep the active chrome — they still need the
            // camera to see that RDYY.
            ProtocolState::Finalized => self.build_finalized_broadcast_screen(title),
            _ => self.build_active_screen(title),
        }
    }

    /// Success chrome with the own-QR broadcast retained — shown from
    /// `Finalized` until `session_ended` (grace expiry drops the strip).
    ///
    /// Mirrors `build_active_screen`'s pinned-QR contract: QR FIRST on a
    /// fixed (non-scrolling) layout so the peer's camera always sees it —
    /// appended below a scrollable summary it lands below the fold exactly
    /// when the peer needs it. The rich success summary is deferred to
    /// `session_ended` for the same reason. The camera is omitted
    /// (post-Finalized scans are no-ops), and Done is styled Secondary:
    /// it tears the broadcast down early while the caption asks the user
    /// to hold position.
    fn build_finalized_broadcast_screen(&self, title: String) -> ScreenModel {
        let mut components: Vec<Component> = Vec::new();
        if let Some(data) = &self.current_qr_data {
            components.push(Component::QrCode {
                id: COMPONENT_ID_OWN_QR.into(),
                data: data.clone(),
                // Animated-QR frame carrier (ADR-044 Am2a C2a); see module doc.
                frames: vec![data.clone()],
                mode: QrMode::Display,
                label: Some(self.t("multi_stage.qr_broadcast_label")),
                scan_quality: None,
                a11y: None,
            });
        }
        components.push(Component::StatusIndicator {
            id: COMPONENT_ID_STATUS.into(),
            icon: Some("checkmark.circle".into()),
            title: self.t("exchange.terminal.complete"),
            detail: None,
            status: Status::Success,
            status_label: self.t(Status::Success.label_key()),
            a11y: None,
        });
        let detail = self
            .peer_name
            .as_ref()
            .map(|name| {
                get_string_with_args(
                    self.locale,
                    "multi_stage.exchanged_with_detail",
                    &[("name", name)],
                )
            })
            .unwrap_or_else(|| self.t("multi_stage.exchange_complete_detail"));
        components.push(Component::Text {
            id: COMPONENT_ID_PEER_NAME.into(),
            content: detail,
            style: TextStyle::Body,
        });

        let mut screen = ScreenModel::new(
            SCREEN_ID,
            title,
            components,
            vec![ScreenAction {
                id: DONE_ACTION_ID.into(),
                label: self.t("action.done"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.done"))),
            }],
        );
        screen.layout = ScreenLayout::Fixed;
        screen
    }

    /// Active rendering — both QR (own card display) and camera
    /// (peer scan) are composed; the status indicator narrates the
    /// current `ProtocolState`.
    fn build_active_screen(&self, title: String) -> ScreenModel {
        let mut components: Vec<Component> = Vec::new();

        // Own QR — pinned at the TOP of a fixed (non-scrolling) layout.
        // It is what the peer scans, so it must never move; keeping it
        // above the status slot means status-height changes can't shift
        // it. While the session is still in `Idle` (no payload yet) we
        // omit it so the frontend shows a chrome-only loading indicator
        // instead of an empty box.
        if let Some(data) = &self.current_qr_data {
            components.push(Component::QrCode {
                id: COMPONENT_ID_OWN_QR.into(),
                data: data.clone(),
                // Animated-QR frame carrier (ADR-044 Am2a C2a); see module doc.
                frames: vec![data.clone()],
                mode: QrMode::Display,
                // The QR label doubles as the exchange status: "Show this"
                // while waiting, then the live progress once the exchange is
                // running (e.g. "Transferring 3/5"). Folding the status into
                // the QR caption lets the non-scrolling layout fit the
                // full-width QR + camera + buttons on a compact screen — there
                // is no separate status row to push them off-screen.
                label: Some(own_qr_label(&self.state, self.locale)),
                scan_quality: None,
                a11y: None,
            });
        }

        // Peer scanner + action buttons share one `Row` so the screen
        // fits the viewport without scrolling (`ScreenLayout::Fixed`):
        // the preview flexes, the buttons take their natural width. The
        // buttons live in the row's `ActionList` rather than the
        // screen-level `actions` (which stay empty); their taps are
        // normalised back to action dispatch in `handle_action`.
        let scan = Component::QrCode {
            id: COMPONENT_ID_PEER_SCAN.into(),
            data: String::new(),
            frames: Vec::new(),
            mode: QrMode::Scan,
            label: Some(self.t("exchange.ble.glance_scan")),
            scan_quality: Some(self.scan_quality_tracker.quality()),
            a11y: None,
        };
        let switch_camera_label = if self.use_front_camera {
            self.t("multi_stage.use_rear_camera_button")
        } else {
            self.t("multi_stage.use_front_camera_button")
        };
        let buttons = Component::ActionList {
            id: EXCHANGE_ACTIONS_ID.into(),
            items: vec![
                ActionListItem {
                    id: SWITCH_CAMERA_ACTION_ID.into(),
                    label: switch_camera_label.clone(),
                    icon: None,
                    detail: None,
                    a11y: Some(A11y::labeled(switch_camera_label)),
                    info_key: None,
                },
                ActionListItem {
                    id: CANCEL_ACTION_ID.into(),
                    label: self.t("action.cancel"),
                    icon: None,
                    detail: None,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                    info_key: None,
                },
            ],
        };
        components.push(Component::Row {
            id: EXCHANGE_PREVIEW_ROW_ID.into(),
            items: vec![scan, buttons],
        });

        let mut screen = ScreenModel::new(SCREEN_ID, title, components, Vec::new());
        screen.layout = ScreenLayout::Fixed;
        screen
    }

    fn build_success_screen(&self, title: String) -> ScreenModel {
        // Rich, core-driven success screen (received card + group +
        // visibility) when the AppEngine attached a summary; otherwise
        // the minimal completion chrome below.
        if let Some(summary) = &self.success_summary {
            return crate::ui::exchange::success::build_exchange_success_screen(
                SCREEN_ID,
                title,
                DONE_ACTION_ID,
                summary,
                self.locale,
            );
        }
        let detail = self
            .peer_name
            .as_ref()
            .map(|name| {
                get_string_with_args(
                    self.locale,
                    "multi_stage.exchanged_with_detail",
                    &[("name", name)],
                )
            })
            .unwrap_or_else(|| self.t("multi_stage.exchange_complete_detail"));

        ScreenModel::new(
            SCREEN_ID,
            title,
            vec![
                Component::StatusIndicator {
                    id: COMPONENT_ID_STATUS.into(),
                    icon: Some("checkmark.circle".into()),
                    title: self.t("exchange.terminal.complete"),
                    detail: None,
                    status: Status::Success,
                    status_label: self.t(Status::Success.label_key()),
                    a11y: None,
                },
                Component::Text {
                    id: COMPONENT_ID_PEER_NAME.into(),
                    content: detail,
                    style: TextStyle::Body,
                },
            ],
            vec![ScreenAction {
                id: DONE_ACTION_ID.into(),
                label: self.t("action.done"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.done"))),
            }],
        )
    }

    fn build_failed_screen(&self, title: String, reason: &str) -> ScreenModel {
        ScreenModel::new(
            SCREEN_ID,
            title,
            vec![Component::StatusIndicator {
                id: COMPONENT_ID_STATUS.into(),
                icon: Some("xmark.circle".into()),
                title: self.t("exchange.terminal.failed_status"),
                detail: Some(reason.to_string()),
                status: Status::Failed,
                status_label: self.t(Status::Failed.label_key()),
                a11y: None,
            }],
            vec![
                ScreenAction {
                    id: RETRY_ACTION_ID.into(),
                    label: self.t("action.retry"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.retry"))),
                },
                ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
        )
    }

    /// TapHoverShake mirror of [`Self::build_audio_failed_screen`].
    /// Distinct chrome from both generic protocol-Failed and
    /// audio-Failed: "Couldn't confirm the shake" tells the user the
    /// accelerometer cross-correlation didn't pass — an actionable
    /// physical-setup hint (shake both phones together). Reached only
    /// when `accel_proximity == Failed` and `audio_proximity != Failed`.
    fn build_accel_failed_screen(&self, title: String) -> ScreenModel {
        ScreenModel::new(
            SCREEN_ID,
            title,
            vec![Component::StatusIndicator {
                id: COMPONENT_ID_STATUS.into(),
                icon: Some("move.3d".into()),
                title: self.t("multi_stage.shake_not_confirmed_title"),
                detail: Some(self.t("multi_stage.shake_not_confirmed_detail")),
                status: Status::Failed,
                status_label: self.t(Status::Failed.label_key()),
                a11y: None,
            }],
            vec![
                ScreenAction {
                    id: RETRY_ACTION_ID.into(),
                    label: self.t("action.retry"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.retry"))),
                },
                ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
        )
    }

    /// G1.3 of the Hover graduation problem record. Distinct chrome
    /// from generic protocol-Failed: "Couldn't confirm devices are
    /// close" tells the user the audio-proximity handshake timed out,
    /// which is a *physical-setup* problem — they should move the
    /// devices closer and retry rather than wonder what "Exchange
    /// Failed" means.
    ///
    /// Retry semantics differ between protocol-Failed and audio-Failed:
    /// the audio-failed retry should restart only the audio verifier
    /// (no QR-cycle restart). That's a session-side concern (Phase
    /// 1.C.3 under Option B), so the action surface remains the same
    /// as the generic Failed screen for now — the handler in
    /// `handle_action` distinguishes by inspecting
    /// `self.audio_proximity` at retry time and emits the appropriate
    /// command set when the session-side work lands.
    fn build_audio_failed_screen(&self, title: String) -> ScreenModel {
        ScreenModel::new(
            SCREEN_ID,
            title,
            vec![Component::StatusIndicator {
                id: COMPONENT_ID_STATUS.into(),
                icon: Some("dot.radiowaves.left.and.right".into()),
                title: self.t("multi_stage.proximity_not_confirmed_title"),
                detail: Some(self.t("multi_stage.proximity_not_confirmed_detail")),
                status: Status::Failed,
                status_label: self.t(Status::Failed.label_key()),
                a11y: None,
            }],
            vec![
                ScreenAction {
                    id: RETRY_ACTION_ID.into(),
                    label: self.t("action.retry"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.retry"))),
                },
                ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
        )
    }
}

/// The own-QR caption, which doubles as the exchange status.
///
/// "Show this" while waiting for a peer, then a short live-progress
/// string once the exchange is running (e.g. "Transferring 3/5"). Folding
/// the status into the QR label replaced the separate `StatusIndicator`
/// row so the non-scrolling exchange layout fits the full-width QR +
/// camera + buttons on a compact screen. Pure helper so frontend tests
/// assert on the same per-state mapping the engine emits.
///
/// Proximity (audio/accel) narration is intentionally not surfaced here —
/// the caption stays short enough to sit under the QR.
pub(crate) fn own_qr_label(state: &ProtocolState, locale: Locale) -> String {
    match state {
        ProtocolState::Idle | ProtocolState::Advertising => {
            get_string(locale, "multi_stage.own_qr_show_this")
        }
        ProtocolState::Discovered => get_string(locale, "multi_stage.own_qr_connecting"),
        ProtocolState::Transferring {
            chunks_sent,
            chunks_total,
            ..
        } => {
            if *chunks_total > 0 {
                get_string_with_args(
                    locale,
                    "multi_stage.own_qr_transferring_progress",
                    &[
                        ("sent", &chunks_sent.to_string()),
                        ("total", &chunks_total.to_string()),
                    ],
                )
            } else {
                get_string(locale, "multi_stage.own_qr_transferring_ellipsis")
            }
        }
        ProtocolState::Verifying => get_string(locale, "multi_stage.own_qr_verifying"),
        ProtocolState::Confirming => get_string(locale, "multi_stage.own_qr_confirming"),
        ProtocolState::Complete | ProtocolState::RetryReady | ProtocolState::Finalized => {
            get_string(locale, "multi_stage.own_qr_almost_done")
        }
        ProtocolState::Failed(_) => get_string(locale, "exchange.failed_title"),
        // ProtocolState is #[non_exhaustive]; future variants surface a
        // generic caption until they get dedicated copy.
        _ => get_string(locale, "multi_stage.own_qr_working"),
    }
}

impl WorkflowEngine for MultiStageExchangeEngine {
    fn current_screen(&self) -> ScreenModel {
        let mut screen = self.build_screen();
        // The multi-stage engine self-clocks via `on_wakeup`/`ScheduleWakeup`
        // (ADR-044 Am2a). The native-wrapper hint is still stamped so shells
        // follow core off the wrapper without matching the `screen_id`.
        screen.native_wrapper_hint = NativeWrapperHint::MultiStageExchange;
        screen
    }

    fn engine_output(&self) -> Option<EngineOutput> {
        Some(EngineOutput::MultiStageExchange {
            hover_mode: self.is_hover_mode(),
        })
    }

    fn apply_update(&mut self, update: EngineUpdate) -> bool {
        let EngineUpdate::MultiStage(update) = update else {
            return false;
        };
        match update {
            MultiStageUpdate::State(state) => self.set_state(state),
            MultiStageUpdate::QrPayload(payload) => self.set_qr_payload(&payload),
            MultiStageUpdate::Finalized(peer_name) => self.set_finalized(peer_name),
            MultiStageUpdate::SuccessSummary(summary) => self.set_success_summary(summary),
            MultiStageUpdate::SessionEnded => self.set_session_ended(),
            MultiStageUpdate::AudioProximity(state) => self.set_audio_proximity(state),
            MultiStageUpdate::AccelProximity(state) => self.set_accel_proximity(state),
        }
        true
    }

    /// Cancel pressed rather than Done after success.
    /// `AppEngine::handle_completion` routes Cancel back to the mode
    /// picker vs Done to Contacts (Fix A of
    /// `2026-06-02-exchange-back-cancel-broken`).
    fn was_cancelled(&self) -> bool {
        self.cancelled
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        let action_id = match action {
            UserAction::ActionPressed { action_id } => action_id,
            // The active screen's switch/cancel buttons live inside the
            // preview `Row`'s `ActionList` (so they sit beside the camera
            // preview); those taps arrive as `ListItemSelected`. Normalise
            // them back to the same action dispatch.
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == EXCHANGE_ACTIONS_ID => item_id,
            _ => return ActionResult::UpdateScreen(self.build_screen()),
        };
        match action_id.as_str() {
            CANCEL_ACTION_ID => {
                self.cancelled = true;
                ActionResult::Complete
            }
            DONE_ACTION_ID => ActionResult::Complete,
            RETRY_ACTION_ID => {
                // Retry returns to the exchange mode-selection picker
                // (AppScreen::Exchange) so the user can re-choose how to
                // exchange — not an in-place restart of the failed mode.
                // `cancelled` is the routing signal `handle_completion`
                // reads to land on the picker instead of Contacts
                // (routing.rs:470-486); the failed session ends and a
                // fresh one spawns on the next mode pick.
                self.cancelled = true;
                ActionResult::Complete
            }
            SWITCH_CAMERA_ACTION_ID => {
                self.use_front_camera = !self.use_front_camera;
                ActionResult::Commands {
                    commands: vec![vauchi_core::Command::SwitchCamera {
                        use_front: self.use_front_camera,
                    }],
                }
            }
            GRANT_CAMERA_PERMISSION_ACTION_ID => {
                // Frontend re-prompts the OS dialog — we optimistically
                // clear the gate; a second `PermissionDenied` event
                // will re-set it if the user denies again.
                self.camera_gate = CameraGate::Available;
                ActionResult::Commands {
                    commands: vec![vauchi_core::Command::QrRequestScan],
                }
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn handle_hardware_event(&mut self, event: Event) -> Option<ActionResult> {
        match event {
            Event::QrScanProgress {
                detected,
                frame_skipped,
                ..
            } => {
                if !frame_skipped {
                    self.scan_quality_tracker.record_frame(detected);
                }
                Some(ActionResult::UpdateScreen(self.build_screen()))
            }
            Event::PermissionDenied { transport } if transport.eq_ignore_ascii_case("camera") => {
                self.camera_gate = self.camera_gate.promote(CameraGate::PermissionDenied);
                Some(ActionResult::UpdateScreen(self.build_screen()))
            }
            Event::HardwareUnavailable { transport }
                if transport.eq_ignore_ascii_case("camera") =>
            {
                self.camera_gate = self.camera_gate.promote(CameraGate::Unavailable);
                Some(ActionResult::UpdateScreen(self.build_screen()))
            }
            _ => None,
        }
    }

    /// 65% brightness keeps the front camera from over-exposing while
    /// scanning the peer's QR (mirror of the prior frontend-side
    /// `UIScreen.main.brightness = 0.65` / Android
    /// `Window.attributes.screenBrightness = 0.65f`). The idle timer
    /// is disabled so a longer-than-30s handshake does not auto-lock.
    /// Orientation locks to portrait so the QR / camera layout stays
    /// stable while the user moves the device — mirrors the prior
    /// `SCREEN_ORIENTATION_PORTRAIT` `DisposableEffect` in
    /// `android/app/src/main/kotlin/app/vauchi/ui/FaceToFaceExchangeScreen.kt`.
    /// Phase 2b + Phase 2c of `2026-05-04-exchange-command-screen-presentation`.
    fn screen_entered(&mut self) -> Vec<vauchi_core::Command> {
        vec![
            vauchi_core::Command::SetScreenBrightness { level: Some(0.65) },
            vauchi_core::Command::SetIdleTimerDisabled { disabled: true },
            vauchi_core::Command::SetOrientationLock {
                orientation: Some(vauchi_core::Orientation::Portrait),
            },
            // Announce the engine's chosen camera selector explicitly so
            // the consumer aligns with engine state on entry rather than
            // coincidentally matching its own back-default. Hover ships
            // `use_front: true` here (face-to-face screen-to-screen UX);
            // Glance keeps `use_front: false`. Phase 1.B of
            // `2026-05-11-hover-graduation-plan.md`.
            vauchi_core::Command::SwitchCamera {
                use_front: self.use_front_camera,
            },
        ]
    }

    /// Symmetric counterpart to [`Self::screen_entered`]: restore the
    /// platform-default brightness (`level: None`), re-enable the
    /// idle timer, and unlock orientation. The frontend's `Command`
    /// handler is responsible for snapshotting the prior brightness on
    /// the first `Some(level)` so the subsequent `None` correctly
    /// restores it (see iOS `AppViewModel::savedBrightness`).
    fn screen_exited(&mut self) -> Vec<vauchi_core::Command> {
        vec![
            vauchi_core::Command::SetScreenBrightness { level: None },
            vauchi_core::Command::SetIdleTimerDisabled { disabled: false },
            vauchi_core::Command::SetOrientationLock { orientation: None },
        ]
    }
}

// INLINE_TEST_REQUIRED: covers private build_screen branches per
// ProtocolState plus the camera-gate fall-through. Cross-crate
// integration in `core/vauchi-app/tests/it/...` could replicate
// surface-level shape but the per-state component layout is the
// engine's contract — keep co-located.
#[cfg(test)]
#[path = "multi_stage_exchange_tests.rs"]
mod tests;
