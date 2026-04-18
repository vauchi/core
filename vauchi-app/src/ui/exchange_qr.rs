// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! QR exchange sub-flow — screen builders and step logic for
//! Glance (QR-only) and Hover (QR + audio proximity) modes.
//!
//! Extracted from `exchange.rs` to keep the orchestrator lean
//! and make room for Link and BLE sub-flows in later tiers.

use crate::ui::*;
use vauchi_core::exchange::ExchangeSession;

// ── Scan quality tracking ──────────────────────────────────────────

/// Window size for the rolling detection rate calculation.
const SCAN_QUALITY_WINDOW: usize = 10;

/// Tracks recent QR scan frame results and computes a [`ScanQuality`]
/// indicator for the viewfinder border color.
///
/// Uses a fixed-size circular buffer of the last [`SCAN_QUALITY_WINDOW`]
/// frames. Detection rate thresholds:
/// - `Good`     (green):  >= 70% detected
/// - `Weak`     (yellow): >= 40% detected
/// - `Poor`     (orange): >= 10% detected
/// - `NoSignal` (red):    < 10% detected
#[derive(Debug, Clone)]
pub(super) struct ScanQualityTracker {
    /// Circular buffer: `true` = QR detected in that frame.
    frames: [bool; SCAN_QUALITY_WINDOW],
    /// Write position in the circular buffer.
    cursor: usize,
    /// Total frames recorded (may exceed window size).
    total: usize,
}

impl ScanQualityTracker {
    pub(super) fn new() -> Self {
        Self {
            frames: [false; SCAN_QUALITY_WINDOW],
            cursor: 0,
            total: 0,
        }
    }

    /// Record a frame result. `detected` = whether a QR was found.
    pub(super) fn record_frame(&mut self, detected: bool) {
        self.frames[self.cursor] = detected;
        self.cursor = (self.cursor + 1) % SCAN_QUALITY_WINDOW;
        self.total += 1;
    }

    /// Current scan quality based on rolling detection rate.
    pub(super) fn quality(&self) -> ScanQuality {
        if self.total == 0 {
            return ScanQuality::NoSignal;
        }

        let window = self.total.min(SCAN_QUALITY_WINDOW);
        let detected_count = if self.total >= SCAN_QUALITY_WINDOW {
            self.frames.iter().filter(|&&d| d).count()
        } else {
            self.frames[..window].iter().filter(|&&d| d).count()
        };

        // Rate as percentage (0-100) to avoid floating point.
        let rate_pct = (detected_count * 100) / window;

        if rate_pct >= 70 {
            ScanQuality::Good
        } else if rate_pct >= 40 {
            ScanQuality::Weak
        } else if rate_pct >= 10 {
            ScanQuality::Poor
        } else {
            ScanQuality::NoSignal
        }
    }

    /// Reset the tracker (e.g., when leaving and re-entering scan mode).
    pub(super) fn reset(&mut self) {
        self.frames = [false; SCAN_QUALITY_WINDOW];
        self.cursor = 0;
        self.total = 0;
    }
}

/// Steps specific to the QR exchange sub-flow.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum QrStep {
    ShowQr,
    ScanQr,
    /// Manual code entry — fallback when camera permission is denied.
    ManualEntry,
    Verifying,
}

impl QrStep {
    /// Step number within the overall exchange flow.
    ///
    /// Offsets by `base` so the QR sub-flow slots into the
    /// parent engine's step numbering (e.g., after group
    /// selection).
    pub(super) fn step_number(&self, base: u8) -> u8 {
        base + match self {
            Self::ShowQr => 0,
            Self::ScanQr | Self::ManualEntry => 1,
            Self::Verifying => 2,
        }
    }

    pub(super) const STEP_COUNT: u8 = 3;
}

/// Builds the "Share Your Code" screen.
pub(super) fn build_show_qr_screen(
    session: Option<&ExchangeSession>,
    config_name: &str,
    config_qr_data: &str,
    progress: Progress,
) -> ScreenModel {
    let qr_data = session
        .and_then(|s| s.qr())
        .map(|qr| qr.to_data_string())
        .unwrap_or_else(|| config_qr_data.to_owned());

    ScreenModel {
        screen_id: "exchange_show_qr".into(),
        title: "Share Your Code".into(),
        subtitle: None,
        components: vec![Component::QrCode {
            id: "own_qr".into(),
            data: qr_data,
            mode: QrMode::Display,
            label: Some(config_name.to_owned()),
            scan_quality: None,
            a11y: Some(A11y {
                label: Some("Your exchange QR code".into()),
                hint: Some("Show this code to the other person to scan".into()),
                role: Some(AccessibilityRole::Image),
            }),
        }],
        actions: vec![ScreenAction {
            id: "continue".into(),
            label: "Scan Their Code".into(),
            style: ActionStyle::Primary,
            enabled: true,
        }],
        progress: Some(progress),
        ..Default::default()
    }
}

/// Builds the "Scan Their Code" screen.
pub(super) fn build_scan_qr_screen(
    progress: Progress,
    scan_quality: Option<ScanQuality>,
) -> ScreenModel {
    ScreenModel {
        screen_id: "exchange_scan_qr".into(),
        title: "Scan Their Code".into(),
        subtitle: None,
        components: vec![Component::QrCode {
            id: "scan_qr".into(),
            data: String::new(),
            mode: QrMode::Scan,
            label: None,
            scan_quality,
            a11y: Some(A11y {
                label: Some("QR code scanner".into()),
                hint: Some("Point the camera at the other person's QR code".into()),
                role: Some(AccessibilityRole::Image),
            }),
        }],
        actions: vec![ScreenAction {
            id: "back".into(),
            label: "Back".into(),
            style: ActionStyle::Secondary,
            enabled: true,
        }],
        progress: Some(progress),
        ..Default::default()
    }
}

/// Builds the "Enter Code Manually" screen — fallback when camera is unavailable.
pub(super) fn build_manual_entry_screen(progress: Progress) -> ScreenModel {
    ScreenModel {
        screen_id: "exchange_manual_entry".into(),
        title: "Enter Code Manually".into(),
        subtitle: Some("Camera unavailable — ask the other person to read their code".into()),
        components: vec![Component::TextInput {
            id: "manual_code".into(),
            label: "Exchange code".into(),
            value: String::new(),
            placeholder: Some("Paste or type the code".into()),
            max_length: None,
            validation_error: None,
            input_type: InputType::Text,
            a11y: Some(A11y {
                label: Some("Exchange code input".into()),
                hint: Some("Enter the exchange code shown on the other person's screen".into()),
                role: Some(AccessibilityRole::TextField),
            }),
            info_key: None,
        }],
        actions: vec![
            ScreenAction {
                id: "submit_code".into(),
                label: "Submit".into(),
                style: ActionStyle::Primary,
                enabled: true,
            },
            ScreenAction {
                id: "back".into(),
                label: "Back".into(),
                style: ActionStyle::Secondary,
                enabled: true,
            },
        ],
        progress: Some(progress),
        ..Default::default()
    }
}

/// Builds the "Verifying" screen.
pub(super) fn build_verifying_screen(progress: Progress) -> ScreenModel {
    ScreenModel {
        screen_id: "exchange_verifying".into(),
        title: "Verifying".into(),
        subtitle: None,
        components: vec![Component::StatusIndicator {
            id: "verifying_status".into(),
            icon: None,
            title: "Verifying...".into(),
            detail: None,
            status: Status::InProgress,
            a11y: Some(A11y {
                label: Some("Verifying exchange".into()),
                hint: Some("Confirming the other person's identity".into()),
                role: None,
            }),
        }],
        actions: vec![],
        progress: Some(progress),
        ..Default::default()
    }
}

/// Handle a user action while in a QR sub-flow step.
///
/// Returns `Some(result)` if the action was handled, `None` if
/// it should fall through to the parent engine.
pub(super) fn handle_qr_action(
    step: &QrStep,
    action: &UserAction,
    session_active: bool,
) -> Option<QrActionOutcome> {
    match (step, action) {
        (QrStep::ShowQr, UserAction::ActionPressed { action_id }) if action_id == "continue" => {
            Some(QrActionOutcome::AdvanceToScan { session_active })
        }
        (QrStep::ScanQr, UserAction::ActionPressed { action_id }) if action_id == "back" => {
            Some(QrActionOutcome::BackToShowQr)
        }
        (
            QrStep::ScanQr,
            UserAction::TextChanged {
                component_id,
                value,
            },
        ) if component_id == "scanned_data" => Some(QrActionOutcome::QrScanned {
            data: value.clone(),
        }),
        // Manual entry: submit the typed code
        (QrStep::ManualEntry, UserAction::ActionPressed { action_id })
            if action_id == "submit_code" =>
        {
            None
        } // Handled by parent via TextChanged
        (QrStep::ManualEntry, UserAction::ActionPressed { action_id }) if action_id == "back" => {
            Some(QrActionOutcome::BackToShowQr)
        }
        (
            QrStep::ManualEntry,
            UserAction::TextChanged {
                component_id,
                value,
            },
        ) if component_id == "manual_code" => Some(QrActionOutcome::ManualCodeEntered {
            data: value.clone(),
        }),
        _ => None,
    }
}

/// Outcome of a QR sub-flow action, interpreted by the parent engine.
pub(super) enum QrActionOutcome {
    /// User pressed "Scan Their Code" — advance to ScanQr step.
    AdvanceToScan { session_active: bool },
    /// User pressed "Back" on scan/manual screen — return to ShowQr.
    BackToShowQr,
    /// User scanned a QR code — store data and move to Verifying.
    QrScanned { data: String },
    /// User submitted a code via manual entry (camera permission denied fallback).
    ManualCodeEntered { data: String },
}

// INLINE_TEST_REQUIRED: tests need pub(super) ScanQualityTracker and screen builder access
#[cfg(test)]
mod tests {
    use super::*;

    // ── ScanQualityTracker ──────────────────────────────────────────

    // @internal
    #[test]
    fn empty_tracker_returns_no_signal() {
        let tracker = ScanQualityTracker::new();
        assert_eq!(tracker.quality(), ScanQuality::NoSignal);
    }

    // @internal
    #[test]
    fn all_detected_returns_good() {
        let mut tracker = ScanQualityTracker::new();
        for _ in 0..10 {
            tracker.record_frame(true);
        }
        assert_eq!(tracker.quality(), ScanQuality::Good);
    }

    // @internal
    #[test]
    fn all_missed_returns_no_signal() {
        let mut tracker = ScanQualityTracker::new();
        for _ in 0..10 {
            tracker.record_frame(false);
        }
        assert_eq!(tracker.quality(), ScanQuality::NoSignal);
    }

    // @internal
    #[test]
    fn seventy_percent_detection_is_good() {
        let mut tracker = ScanQualityTracker::new();
        // 7 detected, 3 missed
        for i in 0..10 {
            tracker.record_frame(i < 7);
        }
        assert_eq!(tracker.quality(), ScanQuality::Good);
    }

    // @internal
    #[test]
    fn sixty_percent_detection_is_weak() {
        let mut tracker = ScanQualityTracker::new();
        // 6 detected, 4 missed
        for i in 0..10 {
            tracker.record_frame(i < 6);
        }
        assert_eq!(tracker.quality(), ScanQuality::Weak);
    }

    // @internal
    #[test]
    fn forty_percent_detection_is_weak() {
        let mut tracker = ScanQualityTracker::new();
        // 4 detected, 6 missed
        for i in 0..10 {
            tracker.record_frame(i < 4);
        }
        assert_eq!(tracker.quality(), ScanQuality::Weak);
    }

    // @internal
    #[test]
    fn twenty_percent_detection_is_poor() {
        let mut tracker = ScanQualityTracker::new();
        // 2 detected, 8 missed
        for i in 0..10 {
            tracker.record_frame(i < 2);
        }
        assert_eq!(tracker.quality(), ScanQuality::Poor);
    }

    // @internal
    #[test]
    fn single_detection_with_partial_window() {
        let mut tracker = ScanQualityTracker::new();
        tracker.record_frame(true);
        // 1/1 = 100% -> Good
        assert_eq!(tracker.quality(), ScanQuality::Good);
    }

    // @internal
    #[test]
    fn rolling_window_drops_old_frames() {
        let mut tracker = ScanQualityTracker::new();
        // Fill with 10 detected frames -> Good
        for _ in 0..10 {
            tracker.record_frame(true);
        }
        assert_eq!(tracker.quality(), ScanQuality::Good);

        // Now push 10 missed frames -> overwrite the window
        for _ in 0..10 {
            tracker.record_frame(false);
        }
        assert_eq!(tracker.quality(), ScanQuality::NoSignal);
    }

    // @internal
    #[test]
    fn reset_clears_state() {
        let mut tracker = ScanQualityTracker::new();
        for _ in 0..5 {
            tracker.record_frame(true);
        }
        assert_eq!(tracker.quality(), ScanQuality::Good);

        tracker.reset();
        assert_eq!(tracker.quality(), ScanQuality::NoSignal);
    }

    // @internal
    #[test]
    fn partial_window_boundary_at_nine_percent() {
        let mut tracker = ScanQualityTracker::new();
        // 9 missed, 0 detected out of 9 frames -> 0% -> NoSignal
        for _ in 0..9 {
            tracker.record_frame(false);
        }
        assert_eq!(tracker.quality(), ScanQuality::NoSignal);

        // 1 detected -> 1/10 = 10% -> Poor
        tracker.record_frame(true);
        assert_eq!(tracker.quality(), ScanQuality::Poor);
    }

    // ── build_scan_qr_screen ────────────────────────────────────────

    // @internal
    #[test]
    fn scan_screen_includes_quality_when_provided() {
        let progress = Progress {
            current_step: 2,
            total_steps: 5,
            label: None,
        };
        let screen = build_scan_qr_screen(progress, Some(ScanQuality::Good));
        let qr = &screen.components[0];
        match qr {
            Component::QrCode { scan_quality, .. } => {
                assert_eq!(*scan_quality, Some(ScanQuality::Good));
            }
            other => panic!("expected QrCode, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn scan_screen_quality_is_none_when_not_provided() {
        let progress = Progress {
            current_step: 2,
            total_steps: 5,
            label: None,
        };
        let screen = build_scan_qr_screen(progress, None);
        let qr = &screen.components[0];
        match qr {
            Component::QrCode { scan_quality, .. } => {
                assert_eq!(*scan_quality, None);
            }
            other => panic!("expected QrCode, got {:?}", other),
        }
    }

    // @internal
    #[test]
    fn show_qr_screen_has_no_scan_quality() {
        let progress = Progress {
            current_step: 1,
            total_steps: 5,
            label: None,
        };
        let screen = build_show_qr_screen(None, "Alice", "qr-data", progress);
        let qr = &screen.components[0];
        match qr {
            Component::QrCode { scan_quality, .. } => {
                assert_eq!(*scan_quality, None);
            }
            other => panic!("expected QrCode, got {:?}", other),
        }
    }
}
