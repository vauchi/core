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

use vauchi_core::Event;
use vauchi_core::exchange::{AudioProximityState, ProtocolState, QrPayload};

use crate::ui::exchange_qr::ScanQualityTracker;
use crate::ui::*;

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
const COMPONENT_ID_STATUS: &str = "status";
const COMPONENT_ID_PEER_NAME: &str = "peer_name";
const COMPONENT_ID_PERMISSION: &str = "permission_required";
const COMPONENT_ID_HARDWARE: &str = "hardware_unavailable";

// ── Screen IDs ─────────────────────────────────────────────────────

/// The single screen id this engine owns. Sub-screens (success
/// overlay, failure overlay, permission gate) all share the id —
/// frontends differentiate by inspecting the components list.
pub const SCREEN_ID: &str = "multi_stage_exchange";

/// Camera reason flags in priority order — permission denied wins
/// over hardware unavailable (per investigation §3.1: a denied
/// permission is recoverable while missing hardware is not, but the
/// user should see the actionable affordance first).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CameraGate {
    #[default]
    Available,
    /// OS permission was denied; the frontend can re-prompt via
    /// `GRANT_CAMERA_PERMISSION_ACTION_ID`.
    PermissionDenied,
    /// Hardware reported absent or unusable. No re-prompt path.
    Unavailable,
}

impl CameraGate {
    /// Returns the gate that should win when a new transport-level
    /// signal arrives. Permission-denied beats already-set unavailable
    /// (more actionable) and vice versa never downgrades from
    /// `Unavailable` to `PermissionDenied` — once hardware is gone the
    /// user cannot grant their way out.
    pub(crate) fn promote(self, incoming: CameraGate) -> CameraGate {
        match (self, incoming) {
            (CameraGate::Unavailable, _) => CameraGate::Unavailable,
            (_, CameraGate::Unavailable) => CameraGate::Unavailable,
            (_, CameraGate::PermissionDenied) => CameraGate::PermissionDenied,
            (current, CameraGate::Available) => current,
        }
    }
}

#[cfg(test)]
mod camera_gate_tests {
    use super::*;

    // @internal
    #[test]
    fn promote_unavailable_is_terminal() {
        let g = CameraGate::Unavailable.promote(CameraGate::PermissionDenied);
        assert_eq!(g, CameraGate::Unavailable);
        let g = CameraGate::Unavailable.promote(CameraGate::Available);
        assert_eq!(g, CameraGate::Unavailable);
    }

    // @internal
    #[test]
    fn promote_permission_denied_replaces_available() {
        let g = CameraGate::Available.promote(CameraGate::PermissionDenied);
        assert_eq!(g, CameraGate::PermissionDenied);
    }

    // @internal
    #[test]
    fn promote_unavailable_overrides_permission_denied() {
        let g = CameraGate::PermissionDenied.promote(CameraGate::Unavailable);
        assert_eq!(g, CameraGate::Unavailable);
    }

    // @internal
    #[test]
    fn promote_available_is_no_op_for_existing_gate() {
        let g = CameraGate::PermissionDenied.promote(CameraGate::Available);
        assert_eq!(g, CameraGate::PermissionDenied);
    }
}

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
}

impl Default for MultiStageExchangeEngine {
    fn default() -> Self {
        Self::new()
    }
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
            session_ended: false,
            camera_gate: CameraGate::Available,
            use_front_camera: false,
            scan_quality_tracker: ScanQualityTracker::new(),
            cancelled: false,
            audio_proximity: AudioProximityState::Pending,
        }
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
            session_ended: false,
            camera_gate: CameraGate::Available,
            use_front_camera: true,
            scan_quality_tracker: ScanQualityTracker::new(),
            cancelled: false,
            audio_proximity: AudioProximityState::Pending,
        }
    }

    /// Backward-compatible shim so existing callers stay green
    /// during the Phase 1.A → Phase 1.B → Phase 1.C tidy / red /
    /// green migration. Delegates to [`Self::new_glance`].
    pub fn new() -> Self {
        Self::new_glance()
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

    /// Current audio-proximity state — `Pending` for Glance throughout
    /// the exchange (the mode never transitions the field). For Hover,
    /// transitions through `Listening → Confirmed` on success or
    /// → `Failed` on the proximity timeout.
    pub fn audio_proximity(&self) -> AudioProximityState {
        self.audio_proximity
    }

    // ── Internal helpers ───────────────────────────────────────────

    fn build_screen(&self) -> ScreenModel {
        let title = "Exchange".to_string();

        // Permission/hardware gate trumps protocol-state chrome — the
        // user cannot make progress without the camera.
        if matches!(self.camera_gate, CameraGate::PermissionDenied) {
            return ScreenModel::new(
                SCREEN_ID,
                title,
                vec![Component::StatusIndicator {
                    id: COMPONENT_ID_PERMISSION.into(),
                    icon: Some("camera.slash".into()),
                    title: "Camera Required".into(),
                    detail: Some(
                        "Tap Grant Permission to allow camera access for the exchange.".into(),
                    ),
                    status: Status::Warning,
                    a11y: None,
                }],
                vec![
                    ScreenAction {
                        id: GRANT_CAMERA_PERMISSION_ACTION_ID.into(),
                        label: "Grant Permission".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                        a11y: None,
                    },
                    ScreenAction {
                        id: CANCEL_ACTION_ID.into(),
                        label: "Cancel".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: None,
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
                    title: "Camera Unavailable".into(),
                    detail: Some("This device cannot scan QR codes for exchange.".into()),
                    status: Status::Failed,
                    a11y: None,
                }],
                vec![ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
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
        match &self.state {
            ProtocolState::Failed(reason) => self.build_failed_screen(title, reason),
            ProtocolState::Finalized | ProtocolState::Complete | ProtocolState::RetryReady
                if self.session_ended =>
            {
                self.build_success_screen(title)
            }
            _ => self.build_active_screen(title),
        }
    }

    /// Active rendering — both QR (own card display) and camera
    /// (peer scan) are composed; the status indicator narrates the
    /// current `ProtocolState`.
    fn build_active_screen(&self, title: String) -> ScreenModel {
        let mut components: Vec<Component> = Vec::new();

        // Own QR — rendered while we have payload data from the cycle
        // thread. While the session is still in `Idle` (start has
        // not produced a payload yet) we omit the QR component
        // entirely so the frontend renders a chrome-only loading
        // indicator instead of an empty box.
        if let Some(data) = &self.current_qr_data {
            components.push(Component::QrCode {
                id: COMPONENT_ID_OWN_QR.into(),
                data: data.clone(),
                mode: QrMode::Display,
                label: Some("Show this to your contact".into()),
                scan_quality: None,
                a11y: None,
            });
        }

        // Peer scanner — always present in Active rendering. Frontend
        // composes camera preview + this overlay; the `scan_quality`
        // value drives the viewfinder border colour.
        components.push(Component::QrCode {
            id: COMPONENT_ID_PEER_SCAN.into(),
            data: String::new(),
            mode: QrMode::Scan,
            label: Some("Scan their code".into()),
            scan_quality: Some(self.scan_quality_tracker.quality()),
            a11y: None,
        });

        // Status narration — derived from ProtocolState plus the
        // active audio-proximity layer (Hover only; Glance stays at
        // Pending so the narration is unchanged for that mode).
        let status = build_status_indicator(&self.state, self.audio_proximity);
        components.push(status);

        let mut actions = vec![ScreenAction {
            id: SWITCH_CAMERA_ACTION_ID.into(),
            label: if self.use_front_camera {
                "Use Rear Camera".into()
            } else {
                "Use Front Camera".into()
            },
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        }];
        actions.push(ScreenAction {
            id: CANCEL_ACTION_ID.into(),
            label: "Cancel".into(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        });

        ScreenModel::new(SCREEN_ID, title, components, actions)
    }

    fn build_success_screen(&self, title: String) -> ScreenModel {
        let detail = self
            .peer_name
            .as_ref()
            .map(|name| format!("Exchanged with {name}"))
            .unwrap_or_else(|| "Exchange complete.".into());

        ScreenModel::new(
            SCREEN_ID,
            title,
            vec![
                Component::StatusIndicator {
                    id: COMPONENT_ID_STATUS.into(),
                    icon: Some("checkmark.circle".into()),
                    title: "Exchange Complete".into(),
                    detail: None,
                    status: Status::Success,
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
                label: "Done".into(),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
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
                title: "Exchange Failed".into(),
                detail: Some(reason.to_string()),
                status: Status::Failed,
                a11y: None,
            }],
            vec![
                ScreenAction {
                    id: RETRY_ACTION_ID.into(),
                    label: "Retry".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
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
                title: "Couldn't confirm devices are close".into(),
                detail: Some("Hold the phones closer together and try again.".to_string()),
                status: Status::Failed,
                a11y: None,
            }],
            vec![
                ScreenAction {
                    id: RETRY_ACTION_ID.into(),
                    label: "Retry".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
        )
    }
}

/// Build the per-state status narration row (active rendering).
///
/// Pure helper so frontend tests assert on the same shape the engine
/// emits and so the per-state mapping stays in one place.
pub(crate) fn build_status_indicator(
    state: &ProtocolState,
    audio: AudioProximityState,
) -> Component {
    let (title, detail, status) = match state {
        ProtocolState::Idle | ProtocolState::Advertising => {
            ("Waiting for peer…", None, Status::Pending)
        }
        ProtocolState::Discovered => (
            "Peer found",
            Some("Starting exchange…".to_string()),
            Status::InProgress,
        ),
        ProtocolState::Transferring {
            chunks_received,
            peer_chunks_total,
            chunks_sent,
            chunks_total,
        } => {
            let detail = if *peer_chunks_total > 0 || *chunks_total > 0 {
                Some(format!(
                    "Receiving {chunks_received}/{peer_chunks_total} · Sending {chunks_sent}/{chunks_total}"
                ))
            } else {
                None
            };
            ("Transferring…", detail, Status::InProgress)
        }
        ProtocolState::Verifying => ("Verifying exchange…", None, Status::InProgress),
        ProtocolState::Confirming => ("Confirming…", None, Status::InProgress),
        ProtocolState::Complete | ProtocolState::RetryReady => (
            "Almost done",
            Some("Keep pointing at the other phone…".to_string()),
            Status::InProgress,
        ),
        ProtocolState::Finalized => (
            "Almost done",
            Some("Keep pointing at the other phone…".to_string()),
            Status::InProgress,
        ),
        ProtocolState::Failed(_) => ("Exchange failed", None, Status::Failed),
        // ProtocolState is #[non_exhaustive]; future variants surface
        // here as a generic in-progress narration until they get a
        // dedicated copy.
        _ => ("Exchange in progress…", None, Status::InProgress),
    };

    // Layer the audio-proximity narration into the detail when the
    // engine is past Pending. Pending (Glance + Hover pre-handshake)
    // leaves the detail untouched; Listening and Confirmed append a
    // user-facing hint about the ongoing ultrasonic handshake. Failed
    // never reaches this helper — `build_screen` routes Audio-Failed
    // to `build_audio_failed_screen` upstream.
    let detail = match audio {
        AudioProximityState::Pending | AudioProximityState::Failed => detail,
        AudioProximityState::Listening => Some(match detail {
            Some(base) => format!("{base} · Listening for proximity chirp"),
            None => "Listening for proximity chirp".to_string(),
        }),
        AudioProximityState::Confirmed => Some(match detail {
            Some(base) => format!("{base} · Devices confirmed close"),
            None => "Devices confirmed close".to_string(),
        }),
    };
    Component::StatusIndicator {
        id: COMPONENT_ID_STATUS.into(),
        icon: None,
        title: title.into(),
        detail,
        status,
        a11y: None,
    }
}

impl WorkflowEngine for MultiStageExchangeEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        let UserAction::ActionPressed { action_id } = action else {
            return ActionResult::UpdateScreen(self.build_screen());
        };
        match action_id.as_str() {
            CANCEL_ACTION_ID => {
                self.cancelled = true;
                ActionResult::Complete
            }
            DONE_ACTION_ID => ActionResult::Complete,
            RETRY_ACTION_ID => {
                // Reset transient state — AppEngine routes Complete to
                // a fresh navigation, but a local Retry needs the engine
                // to forget the failure narrative immediately so the
                // next state push (typically Idle/Advertising) renders
                // cleanly. Re-issuing `QrRequestScan` re-arms the
                // camera pipeline.
                self.state = ProtocolState::Idle;
                self.peer_name = None;
                self.session_ended = false;
                self.scan_quality_tracker.reset();
                ActionResult::Commands {
                    commands: vec![vauchi_core::Command::QrRequestScan],
                }
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

    /// Downcast for the platform-side bridge.
    ///
    /// `core/vauchi-platform/src/platform_app_engine.rs::apply_multi_stage_*`
    /// uses this to mutate the active multi-stage engine via the bridge
    /// setters when `MultiStageSessionListener` callbacks fire from the
    /// cycle thread. No other downcast site is supported.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
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
mod tests {
    use super::*;

    fn engine_with_state(state: ProtocolState) -> MultiStageExchangeEngine {
        let mut e = MultiStageExchangeEngine::new();
        e.set_state(state);
        e
    }

    fn engine_with_qr(state: ProtocolState, data: &str) -> MultiStageExchangeEngine {
        let mut e = MultiStageExchangeEngine::new();
        e.set_state(state);
        e.set_qr_payload(&QrPayload {
            data: data.into(),
            error_correction: "L".into(),
            display_duration_ms: 400,
        });
        e
    }

    fn first_status_indicator(screen: &ScreenModel) -> Option<&Component> {
        screen
            .components
            .iter()
            .find(|c| matches!(c, Component::StatusIndicator { .. }))
    }

    fn action_ids(screen: &ScreenModel) -> Vec<&str> {
        screen.actions.iter().map(|a| a.id.as_str()).collect()
    }

    // ── Mode-aware construction (Phase 1.A) ─────────────────────

    // RED for Phase 1.A.2 of `2026-05-11-hover-graduation-plan.md`.
    // Hover defaults the camera selector to `front` (face-to-face
    // screen-to-screen) and starts with audio proximity `Pending`
    // because the ultrasonic handshake hasn't run yet. The Glance
    // path (`new_glance`) ignores `audio_proximity` and stays
    // back-camera-default.
    // @internal
    #[test]
    fn new_hover_initialises_state() {
        let engine = MultiStageExchangeEngine::new_hover();
        assert!(
            engine.use_front_camera(),
            "Hover engine must default to the front camera",
        );
        assert_eq!(
            engine.audio_proximity(),
            AudioProximityState::Pending,
            "Hover engine must start with audio proximity Pending",
        );
    }

    // @internal
    #[test]
    fn new_glance_is_back_camera_default() {
        let engine = MultiStageExchangeEngine::new_glance();
        assert!(
            !engine.use_front_camera(),
            "Glance engine must default to the back camera",
        );
    }

    // ── Audio-proximity setter + rendering (Phase 1.C.2 + 1.D) ────

    // @internal
    #[test]
    fn set_audio_proximity_transitions_state() {
        let mut engine = MultiStageExchangeEngine::new_hover();
        assert_eq!(engine.audio_proximity(), AudioProximityState::Pending);
        engine.set_audio_proximity(AudioProximityState::Listening);
        assert_eq!(engine.audio_proximity(), AudioProximityState::Listening);
        engine.set_audio_proximity(AudioProximityState::Confirmed);
        assert_eq!(engine.audio_proximity(), AudioProximityState::Confirmed);
    }

    // @internal
    #[test]
    fn set_audio_proximity_is_noop_when_cancelled() {
        let mut engine = MultiStageExchangeEngine::new_hover();
        // Drive into the cancelled state via the same path the engine's
        // user-action handler uses — pressing CANCEL flips the flag.
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: CANCEL_ACTION_ID.into(),
        });
        // Subsequent setter calls must not update the field; the
        // engine ignores late callbacks after the user cancelled.
        engine.set_audio_proximity(AudioProximityState::Confirmed);
        assert_eq!(
            engine.audio_proximity(),
            AudioProximityState::Pending,
            "cancelled engine must reject set_audio_proximity",
        );
    }

    // @internal
    #[test]
    fn audio_proximity_listening_appends_to_status_detail() {
        let mut engine = MultiStageExchangeEngine::new_hover();
        engine.set_state(ProtocolState::Discovered);
        engine.set_audio_proximity(AudioProximityState::Listening);
        let screen = engine.current_screen();
        let status = first_status_indicator(&screen).expect("status indicator");
        let Component::StatusIndicator { detail, .. } = status else {
            panic!("expected StatusIndicator");
        };
        assert!(
            detail
                .as_ref()
                .is_some_and(|d| d.contains("Listening for proximity chirp")),
            "Listening must surface in status detail; got {detail:?}",
        );
    }

    // @internal
    #[test]
    fn audio_proximity_confirmed_appends_to_status_detail() {
        let mut engine = MultiStageExchangeEngine::new_hover();
        engine.set_state(ProtocolState::Discovered);
        engine.set_audio_proximity(AudioProximityState::Confirmed);
        let screen = engine.current_screen();
        let status = first_status_indicator(&screen).expect("status indicator");
        let Component::StatusIndicator { detail, .. } = status else {
            panic!("expected StatusIndicator");
        };
        assert!(
            detail
                .as_ref()
                .is_some_and(|d| d.contains("Devices confirmed close")),
            "Confirmed must surface in status detail; got {detail:?}",
        );
    }

    // @internal
    #[test]
    fn audio_proximity_pending_leaves_status_detail_unchanged() {
        // Glance regression gate: with audio_proximity Pending, the
        // status indicator detail must match the pre-1.C.2 narration
        // for the same protocol state. The "Peer found / Starting
        // exchange…" detail is the ProtocolState::Discovered shape.
        let mut engine = MultiStageExchangeEngine::new_glance();
        engine.set_state(ProtocolState::Discovered);
        let screen = engine.current_screen();
        let status = first_status_indicator(&screen).expect("status indicator");
        let Component::StatusIndicator { detail, .. } = status else {
            panic!("expected StatusIndicator");
        };
        let detail_str = detail.as_deref().unwrap_or("");
        assert!(
            !detail_str.contains("Listening") && !detail_str.contains("Devices confirmed close"),
            "Pending must not surface audio narration; got {detail_str:?}",
        );
    }

    // @internal
    #[test]
    fn audio_proximity_failed_renders_audio_failed_screen() {
        let mut engine = MultiStageExchangeEngine::new_hover();
        engine.set_audio_proximity(AudioProximityState::Failed);
        let screen = engine.current_screen();
        let status = first_status_indicator(&screen).expect("status indicator");
        let Component::StatusIndicator {
            title: status_title,
            ..
        } = status
        else {
            panic!("expected StatusIndicator");
        };
        assert_eq!(
            status_title, "Couldn't confirm devices are close",
            "audio-Failed must render the proximity-specific chrome, not the generic Exchange Failed panel",
        );
        // Retry + Cancel actions are present on the audio-failed
        // screen so the user can attempt the handshake again.
        let ids: Vec<&str> = action_ids(&screen);
        assert!(
            ids.contains(&RETRY_ACTION_ID),
            "audio-failed screen must offer Retry; got {ids:?}",
        );
        assert!(
            ids.contains(&CANCEL_ACTION_ID),
            "audio-failed screen must offer Cancel; got {ids:?}",
        );
    }

    // @internal
    #[test]
    fn audio_failed_takes_precedence_over_protocol_failed() {
        // Both failure modes co-exist on a single engine after a
        // failed handshake: protocol may have failed for an
        // unrelated reason while audio_proximity also went Failed.
        // The user-facing chrome should narrate the audio failure
        // (the actionable physical-setup hint) rather than a
        // generic "Exchange failed" panel.
        let mut engine = MultiStageExchangeEngine::new_hover();
        engine.set_state(ProtocolState::Failed("generic-reason".to_string()));
        engine.set_audio_proximity(AudioProximityState::Failed);
        let screen = engine.current_screen();
        let status = first_status_indicator(&screen).expect("status indicator");
        let Component::StatusIndicator {
            title: status_title,
            ..
        } = status
        else {
            panic!("expected StatusIndicator");
        };
        assert_eq!(
            status_title, "Couldn't confirm devices are close",
            "audio_proximity:Failed must take precedence over ProtocolState::Failed",
        );
    }

    // ── Per-ProtocolState rendering ──────────────────────────────

    // @internal
    #[test]
    fn idle_emits_waiting_status_with_peer_scanner() {
        let engine = MultiStageExchangeEngine::new();
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, SCREEN_ID);
        // Idle without a QR payload yet — no own_qr component.
        assert!(
            !screen
                .components
                .iter()
                .any(|c| matches!(c, Component::QrCode { id, .. } if id == COMPONENT_ID_OWN_QR)),
        );
        // Peer scanner is always present in Active rendering.
        let has_scanner = screen.components.iter().any(|c| {
            matches!(
                c,
                Component::QrCode { id, mode: QrMode::Scan, .. } if id == COMPONENT_ID_PEER_SCAN,
            )
        });
        assert!(has_scanner, "Idle must compose camera scanner");
        let status = first_status_indicator(&screen).expect("status indicator");
        match status {
            Component::StatusIndicator { title, status, .. } => {
                assert_eq!(title, "Waiting for peer…");
                assert_eq!(*status, Status::Pending);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            action_ids(&screen),
            vec![SWITCH_CAMERA_ACTION_ID, CANCEL_ACTION_ID]
        );
    }

    // @internal
    #[test]
    fn advertising_renders_active_with_qr_when_payload_present() {
        let engine = engine_with_qr(ProtocolState::Advertising, "vauchi://INIT/abc");
        let screen = engine.current_screen();
        let has_own_qr = screen.components.iter().any(|c| {
            matches!(
                c,
                Component::QrCode { id, mode: QrMode::Display, data, .. }
                    if id == COMPONENT_ID_OWN_QR && data == "vauchi://INIT/abc",
            )
        });
        assert!(has_own_qr, "Advertising with payload must render own QR");
    }

    // @internal
    #[test]
    fn discovered_state_narrates_starting_exchange() {
        let engine = engine_with_state(ProtocolState::Discovered);
        match first_status_indicator(&engine.current_screen()).unwrap() {
            Component::StatusIndicator {
                title,
                detail,
                status,
                ..
            } => {
                assert_eq!(title, "Peer found");
                assert_eq!(detail.as_deref(), Some("Starting exchange…"));
                assert_eq!(*status, Status::InProgress);
            }
            _ => unreachable!(),
        }
    }

    // @internal
    #[test]
    fn transferring_state_includes_chunk_progress() {
        let engine = engine_with_state(ProtocolState::Transferring {
            chunks_sent: 3,
            chunks_total: 7,
            chunks_received: 5,
            peer_chunks_total: 9,
        });
        match first_status_indicator(&engine.current_screen()).unwrap() {
            Component::StatusIndicator {
                title,
                detail,
                status,
                ..
            } => {
                assert_eq!(title, "Transferring…");
                assert_eq!(detail.as_deref(), Some("Receiving 5/9 · Sending 3/7"),);
                assert_eq!(*status, Status::InProgress);
            }
            _ => unreachable!(),
        }
    }

    // @internal
    #[test]
    fn transferring_with_zero_totals_omits_progress_detail() {
        let engine = engine_with_state(ProtocolState::Transferring {
            chunks_sent: 0,
            chunks_total: 0,
            chunks_received: 0,
            peer_chunks_total: 0,
        });
        match first_status_indicator(&engine.current_screen()).unwrap() {
            Component::StatusIndicator { detail, .. } => {
                assert!(
                    detail.is_none(),
                    "expected no progress detail when totals are zero"
                );
            }
            _ => unreachable!(),
        }
    }

    // @internal
    #[test]
    fn verifying_state_narrates_verifying() {
        let engine = engine_with_state(ProtocolState::Verifying);
        match first_status_indicator(&engine.current_screen()).unwrap() {
            Component::StatusIndicator { title, status, .. } => {
                assert_eq!(title, "Verifying exchange…");
                assert_eq!(*status, Status::InProgress);
            }
            _ => unreachable!(),
        }
    }

    // @internal
    #[test]
    fn confirming_state_narrates_confirming() {
        let engine = engine_with_state(ProtocolState::Confirming);
        match first_status_indicator(&engine.current_screen()).unwrap() {
            Component::StatusIndicator { title, .. } => {
                assert_eq!(title, "Confirming…");
            }
            _ => unreachable!(),
        }
    }

    // @internal
    #[test]
    fn complete_before_session_ended_keeps_active_chrome() {
        let engine = engine_with_state(ProtocolState::Complete);
        match first_status_indicator(&engine.current_screen()).unwrap() {
            Component::StatusIndicator { title, detail, .. } => {
                assert_eq!(title, "Almost done");
                assert_eq!(detail.as_deref(), Some("Keep pointing at the other phone…"));
            }
            _ => unreachable!(),
        }
        // Still active — switch_camera + cancel.
        assert_eq!(
            action_ids(&engine.current_screen()),
            vec![SWITCH_CAMERA_ACTION_ID, CANCEL_ACTION_ID],
        );
    }

    // @internal
    #[test]
    fn finalized_after_session_ended_renders_success() {
        let mut engine = engine_with_state(ProtocolState::Finalized);
        engine.set_finalized("Alice".into());
        engine.set_session_ended();
        let screen = engine.current_screen();
        let has_success = screen.components.iter().any(|c| {
            matches!(
                c,
                Component::StatusIndicator { title, status: Status::Success, .. }
                    if title == "Exchange Complete",
            )
        });
        assert!(
            has_success,
            "session_ended Finalized must show success indicator"
        );
        let has_name = screen.components.iter().any(|c| {
            matches!(
                c,
                Component::Text { content, .. } if content == "Exchanged with Alice",
            )
        });
        assert!(has_name, "success screen must include peer name");
        assert_eq!(action_ids(&screen), vec![DONE_ACTION_ID]);
    }

    // @internal
    #[test]
    fn finalized_with_session_ended_but_no_name_falls_back() {
        let mut engine = engine_with_state(ProtocolState::Finalized);
        engine.set_session_ended();
        let screen = engine.current_screen();
        let has_fallback = screen.components.iter().any(|c| {
            matches!(
                c,
                Component::Text { content, .. } if content == "Exchange complete.",
            )
        });
        assert!(
            has_fallback,
            "missing peer name must fall back to generic copy"
        );
    }

    // @internal
    #[test]
    fn failed_state_renders_retry_and_cancel() {
        let engine = engine_with_state(ProtocolState::Failed("timeout".into()));
        let screen = engine.current_screen();
        match first_status_indicator(&screen).unwrap() {
            Component::StatusIndicator {
                title,
                detail,
                status,
                ..
            } => {
                assert_eq!(title, "Exchange Failed");
                assert_eq!(detail.as_deref(), Some("timeout"));
                assert_eq!(*status, Status::Failed);
            }
            _ => unreachable!(),
        }
        assert_eq!(action_ids(&screen), vec![RETRY_ACTION_ID, CANCEL_ACTION_ID]);
    }

    // ── Camera gate ─────────────────────────────────────────────

    // @internal
    #[test]
    fn permission_denied_event_swaps_to_permission_screen() {
        let mut engine = MultiStageExchangeEngine::new();
        let result = engine.handle_hardware_event(Event::PermissionDenied {
            transport: "Camera".into(),
        });
        assert!(
            result.is_some(),
            "engine must update screen on permission denied"
        );
        let screen = engine.current_screen();
        let title_match = screen.components.iter().any(|c| {
            matches!(
                c,
                Component::StatusIndicator { id, title, .. }
                    if id == COMPONENT_ID_PERMISSION && title == "Camera Required",
            )
        });
        assert!(
            title_match,
            "permission denied must surface Camera Required"
        );
        assert_eq!(
            action_ids(&screen),
            vec![GRANT_CAMERA_PERMISSION_ACTION_ID, CANCEL_ACTION_ID],
        );
    }

    // @internal
    #[test]
    fn hardware_unavailable_event_swaps_to_hardware_screen() {
        let mut engine = MultiStageExchangeEngine::new();
        engine.handle_hardware_event(Event::HardwareUnavailable {
            transport: "camera".into(),
        });
        let screen = engine.current_screen();
        let has_hardware = screen.components.iter().any(|c| {
            matches!(
                c,
                Component::StatusIndicator { id, title, status: Status::Failed, .. }
                    if id == COMPONENT_ID_HARDWARE && title == "Camera Unavailable",
            )
        });
        assert!(has_hardware);
        assert_eq!(action_ids(&screen), vec![CANCEL_ACTION_ID]);
    }

    // @internal
    #[test]
    fn unrelated_transport_does_not_engage_gate() {
        let mut engine = MultiStageExchangeEngine::new();
        engine.handle_hardware_event(Event::PermissionDenied {
            transport: "BLE".into(),
        });
        // Still active rendering — the BLE permission denial does not
        // gate the camera-only flow.
        let screen = engine.current_screen();
        let has_scanner = screen.components.iter().any(|c| {
            matches!(
                c,
                Component::QrCode { id, mode: QrMode::Scan, .. } if id == COMPONENT_ID_PEER_SCAN,
            )
        });
        assert!(
            has_scanner,
            "unrelated transport must not gate the camera screen"
        );
    }

    // ── Action handling ─────────────────────────────────────────

    // @internal
    #[test]
    fn cancel_action_returns_complete_and_blocks_further_state_pushes() {
        let mut engine = MultiStageExchangeEngine::new();
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: CANCEL_ACTION_ID.into(),
        });
        assert!(matches!(result, ActionResult::Complete));
        // Late state push from the cycle thread must not un-cancel.
        engine.set_state(ProtocolState::Finalized);
        engine.set_finalized("Late".into());
        // Engine still considers itself cancelled — state didn't move.
        assert_eq!(engine.state, ProtocolState::Idle);
        assert!(engine.peer_name.is_none());
    }

    // @internal
    #[test]
    fn done_action_returns_complete() {
        let mut engine = engine_with_state(ProtocolState::Finalized);
        engine.set_session_ended();
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: DONE_ACTION_ID.into(),
        });
        assert!(matches!(result, ActionResult::Complete));
    }

    // @internal
    #[test]
    fn retry_action_resets_state_and_re_arms_camera() {
        let mut engine = engine_with_state(ProtocolState::Failed("bad".into()));
        engine.set_finalized("Stale".into());
        engine.scan_quality_tracker.record_frame(true);
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: RETRY_ACTION_ID.into(),
        });
        match result {
            ActionResult::Commands { commands } => {
                assert_eq!(commands.len(), 1);
                assert!(matches!(&commands[0], vauchi_core::Command::QrRequestScan,));
            }
            other => panic!("expected Commands, got {other:?}"),
        }
        assert_eq!(engine.state, ProtocolState::Idle);
        assert!(
            engine.peer_name.is_none(),
            "retry must clear stale peer name"
        );
        assert!(!engine.session_ended);
    }

    // @internal
    #[test]
    fn switch_camera_toggles_state_and_emits_command() {
        let mut engine = MultiStageExchangeEngine::new();
        assert!(!engine.use_front_camera());
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: SWITCH_CAMERA_ACTION_ID.into(),
        });
        match result {
            ActionResult::Commands { commands } => match &commands[0] {
                vauchi_core::Command::SwitchCamera { use_front } => {
                    assert!(use_front, "first toggle must select front");
                }
                other => panic!("expected SwitchCamera, got {other:?}"),
            },
            other => panic!("expected Commands, got {other:?}"),
        }
        assert!(engine.use_front_camera());
        // Toggle back.
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: SWITCH_CAMERA_ACTION_ID.into(),
        });
        assert!(!engine.use_front_camera());
    }

    // @internal
    #[test]
    fn switch_camera_label_reflects_current_orientation() {
        let mut engine = MultiStageExchangeEngine::new();
        let label_rear = engine
            .current_screen()
            .actions
            .into_iter()
            .find(|a| a.id == SWITCH_CAMERA_ACTION_ID)
            .map(|a| a.label)
            .unwrap();
        assert_eq!(label_rear, "Use Front Camera");
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: SWITCH_CAMERA_ACTION_ID.into(),
        });
        let label_front = engine
            .current_screen()
            .actions
            .into_iter()
            .find(|a| a.id == SWITCH_CAMERA_ACTION_ID)
            .map(|a| a.label)
            .unwrap();
        assert_eq!(label_front, "Use Rear Camera");
    }

    // @internal
    #[test]
    fn grant_permission_action_clears_gate_and_re_requests_scan() {
        let mut engine = MultiStageExchangeEngine::new();
        engine.handle_hardware_event(Event::PermissionDenied {
            transport: "camera".into(),
        });
        assert_eq!(engine.camera_gate, CameraGate::PermissionDenied);
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: GRANT_CAMERA_PERMISSION_ACTION_ID.into(),
        });
        assert_eq!(engine.camera_gate, CameraGate::Available);
        match result {
            ActionResult::Commands { commands } => {
                assert!(matches!(&commands[0], vauchi_core::Command::QrRequestScan,));
            }
            other => panic!("expected Commands, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn unavailable_gate_cannot_be_recovered_by_grant_permission() {
        let mut engine = MultiStageExchangeEngine::new();
        engine.handle_hardware_event(Event::HardwareUnavailable {
            transport: "camera".into(),
        });
        // Permission-denied event arrives later — still terminal.
        engine.handle_hardware_event(Event::PermissionDenied {
            transport: "camera".into(),
        });
        assert_eq!(engine.camera_gate, CameraGate::Unavailable);
        // Hardware screen has no Grant Permission affordance.
        let screen = engine.current_screen();
        let ids = action_ids(&screen);
        assert!(!ids.contains(&GRANT_CAMERA_PERMISSION_ACTION_ID));
    }

    // ── QrScanProgress hardware event ───────────────────────────

    // @internal
    #[test]
    fn qr_scan_progress_drives_quality_tracker() {
        let mut engine = MultiStageExchangeEngine::new();
        for _ in 0..10 {
            engine.handle_hardware_event(Event::QrScanProgress {
                detected: true,
                confidence: Some(95),
                frame_skipped: false,
            });
        }
        let screen = engine.current_screen();
        let scan_quality = screen.components.iter().find_map(|c| match c {
            Component::QrCode {
                mode: QrMode::Scan,
                scan_quality,
                ..
            } => Some(*scan_quality),
            _ => None,
        });
        assert_eq!(scan_quality, Some(Some(ScanQuality::Good)));
    }

    // @internal
    #[test]
    fn skipped_frames_do_not_reach_tracker() {
        let mut engine = MultiStageExchangeEngine::new();
        // 10 detected frames — Good.
        for _ in 0..10 {
            engine.handle_hardware_event(Event::QrScanProgress {
                detected: true,
                confidence: None,
                frame_skipped: false,
            });
        }
        // Skipped frames must NOT pollute the rolling rate.
        for _ in 0..20 {
            engine.handle_hardware_event(Event::QrScanProgress {
                detected: false,
                confidence: None,
                frame_skipped: true,
            });
        }
        let screen = engine.current_screen();
        let scan_quality = screen.components.iter().find_map(|c| match c {
            Component::QrCode {
                mode: QrMode::Scan,
                scan_quality,
                ..
            } => *scan_quality,
            _ => None,
        });
        assert_eq!(scan_quality, Some(ScanQuality::Good));
    }

    // ── Adversarial input ───────────────────────────────────────

    // @internal
    #[test]
    fn unknown_action_id_falls_through_to_update_screen() {
        let mut engine = MultiStageExchangeEngine::new();
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "🦀;DROP TABLE".into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
    }

    // @internal
    #[test]
    fn non_action_pressed_user_action_falls_through() {
        let mut engine = MultiStageExchangeEngine::new();
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "x".into(),
            item_id: "y".into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
    }

    // @internal
    #[test]
    fn long_failure_reason_renders_without_truncation() {
        let long = "a".repeat(1024);
        let engine = engine_with_state(ProtocolState::Failed(long.clone()));
        match first_status_indicator(&engine.current_screen()).unwrap() {
            Component::StatusIndicator { detail, .. } => {
                assert_eq!(detail.as_deref(), Some(long.as_str()));
            }
            _ => unreachable!(),
        }
    }

    // ── Screen-presentation lifecycle (Phase 2b) ─────────────────────

    // @scenario: exchange.feature :: Multi-stage exchange (Glance) dims screen, disables idle timer, locks portrait, and announces back camera on entry
    #[test]
    fn screen_entered_glance_emits_presentation_commands_and_back_camera() {
        use vauchi_core::{Command, Orientation};
        let mut engine = MultiStageExchangeEngine::new_glance();
        let commands = engine.screen_entered();
        assert_eq!(
            commands,
            vec![
                Command::SetScreenBrightness { level: Some(0.65) },
                Command::SetIdleTimerDisabled { disabled: true },
                Command::SetOrientationLock {
                    orientation: Some(Orientation::Portrait)
                },
                Command::SwitchCamera { use_front: false },
            ],
            "Glance screen_entered must dim brightness, disable idle timer, lock portrait, and announce back camera"
        );
    }

    // @scenario: exchange.feature :: Multi-stage exchange (Hover) dims screen, disables idle timer, locks portrait, and announces front camera on entry
    #[test]
    fn screen_entered_hover_emits_presentation_commands_and_front_camera() {
        use vauchi_core::{Command, Orientation};
        let mut engine = MultiStageExchangeEngine::new_hover();
        let commands = engine.screen_entered();
        assert_eq!(
            commands,
            vec![
                Command::SetScreenBrightness { level: Some(0.65) },
                Command::SetIdleTimerDisabled { disabled: true },
                Command::SetOrientationLock {
                    orientation: Some(Orientation::Portrait)
                },
                Command::SwitchCamera { use_front: true },
            ],
            "Hover screen_entered must dim brightness, disable idle timer, lock portrait, and announce front camera"
        );
    }

    // @scenario: exchange.feature :: Multi-stage exchange restores presentation defaults on exit
    #[test]
    fn screen_exited_emits_brightness_idle_timer_and_orientation_unlock() {
        use vauchi_core::Command;
        let mut engine = MultiStageExchangeEngine::new();
        let commands = engine.screen_exited();
        assert_eq!(
            commands,
            vec![
                Command::SetScreenBrightness { level: None },
                Command::SetIdleTimerDisabled { disabled: false },
                Command::SetOrientationLock { orientation: None },
            ],
            "screen_exited must restore brightness, idle timer, and orientation defaults"
        );
    }
}
