// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rolling QR-scan-quality tracker (viewfinder border colour).
//!
//! Extracted from `exchange/qr.rs` (2026-06-03) so it outlives the legacy
//! QR sub-flow: the multi-stage exchange engine drives the same tracker for
//! its peer-scanner `scan_quality`. `ScanQuality` (the wire enum) lives in
//! `ui::component`; this module owns only the rolling-rate tracker.

use crate::ui::*;

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
pub(crate) struct ScanQualityTracker {
    /// Circular buffer: `true` = QR detected in that frame.
    frames: [bool; SCAN_QUALITY_WINDOW],
    /// Write position in the circular buffer.
    cursor: usize,
    /// Total frames recorded (may exceed window size).
    total: usize,
}

impl ScanQualityTracker {
    pub(crate) fn new() -> Self {
        Self {
            frames: [false; SCAN_QUALITY_WINDOW],
            cursor: 0,
            total: 0,
        }
    }

    /// Record a frame result. `detected` = whether a QR was found.
    pub(crate) fn record_frame(&mut self, detected: bool) {
        self.frames[self.cursor] = detected;
        self.cursor = (self.cursor + 1) % SCAN_QUALITY_WINDOW;
        self.total += 1;
    }

    /// Current scan quality based on rolling detection rate.
    pub(crate) fn quality(&self) -> ScanQuality {
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
}

// INLINE_TEST_REQUIRED: tests exercise the pub(crate) ScanQualityTracker
// rolling-detection-rate internals (record_frame / quality / reset), not a
// public API surface — kept co-located with the impl.
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
}
