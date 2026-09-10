// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Terminal/interrupt chrome for the multi-stage exchange engine —
//! Failed, Stalled, accel-proximity-Failed, audio-proximity-Failed.
//! Split out of `multi_stage_exchange.rs` when that file crossed the
//! file-size threshold (`_private/docs/backlog/
//! 2026-09-10-exchange-stall-and-ble-fallback-states/README.md`): these
//! four screen builders have no dependency on each other or on the
//! active/success builders, so they split cleanly. Loaded via
//! `#[path]`; stays a child module (`super::` private-field access
//! preserved — `pub(super)` on each fn so `build_screen` can still call
//! them as inherent methods).

use super::*;

impl MultiStageExchangeEngine {
    pub(super) fn build_failed_screen(&self, title: String, reason: &str) -> ScreenModel {
        ScreenModel::new(
            SCREEN_ID,
            title,
            vec![
                Component::StatusIndicator {
                    id: COMPONENT_ID_STATUS.into(),
                    icon: Some("xmark.circle".into()),
                    title: self.t("exchange.terminal.failed_status"),
                    detail: Some(reason.to_string()),
                    status: Status::Failed,
                    status_label: self.t(Status::Failed.label_key()),
                    a11y: Some(A11y {
                        label: Some(self.t("exchange.terminal.failed_status")),
                        hint: Some(self.t("exchange.terminal.failed_hint")),
                        role: None,
                    }),
                },
                // States plainly that the failed attempt saved nothing —
                // a dead session must not be mistaken for one that partly
                // succeeded (`2026-08-07-ble-exchange-ui-never-shows-completion`
                // is the inverse defect: a live session that looked dead).
                Component::Text {
                    a11y: None,
                    id: "failed_nothing_saved".into(),
                    content: self.t("exchange.terminal.failed_nothing_saved"),
                    style: TextStyle::Caption,
                },
            ],
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

    /// Peer-frame stall presentation (`_private/docs/backlog/
    /// 2026-09-10-exchange-stall-and-ble-fallback-states/README.md`).
    /// Reuses `build_active_screen`'s own-QR and peer-scan components so
    /// scanning keeps running: a newly decoded frame is exactly what
    /// clears the stall, so the camera cannot be dropped from the screen
    /// the way a terminal chrome would drop it. Only the banner and the
    /// bottom action row are Stalled-specific.
    pub(super) fn build_stalled_screen(&self, title: String) -> ScreenModel {
        let mut screen = self.build_active_screen(title);
        screen.components.insert(
            0,
            Component::StatusIndicator {
                id: "stalled_status".into(),
                icon: Some("clock.arrow.circlepath".into()),
                title: self.t("exchange.stalled_title"),
                // Keeps the frame current/total progress visible via the
                // existing transferring-progress strings rather than
                // duplicating that formatting in a new locale key.
                detail: Some(own_qr_label(&self.state, self.locale)),
                status: Status::Warning,
                status_label: self.t(Status::Warning.label_key()),
                a11y: Some(A11y {
                    label: Some(self.t("exchange.stalled_title")),
                    hint: Some(self.t("exchange.stalled_detail")),
                    role: None,
                }),
            },
        );
        screen.contextual_actions = vec![
            ScreenAction {
                id: RETRY_ACTION_ID.into(),
                label: self.t("action.retry"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.retry"))),
            },
            ScreenAction {
                id: SWITCH_RELAY_ACTION_ID.into(),
                label: self.t("exchange.terminal.switch_relay"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y {
                    label: None,
                    hint: Some(self.t("exchange.terminal.switch_relay_hint")),
                    role: None,
                }),
            },
        ];
        screen
    }

    /// TapHoverShake mirror of [`Self::build_audio_failed_screen`].
    /// Distinct chrome from both generic protocol-Failed and
    /// audio-Failed: "Couldn't confirm the shake" tells the user the
    /// accelerometer cross-correlation didn't pass — an actionable
    /// physical-setup hint (shake both phones together). Reached only
    /// when `accel_proximity == Failed` and `audio_proximity != Failed`.
    pub(super) fn build_accel_failed_screen(&self, title: String) -> ScreenModel {
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
    pub(super) fn build_audio_failed_screen(&self, title: String) -> ScreenModel {
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
