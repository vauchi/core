// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Link training: where on the screen the next code is drawn.
//!
//! Two phones held face to face each see only part of the other's screen,
//! and which part differs per pair and per distance. Each phone therefore
//! tells the other, in every frame's [`TrainingHeader`], how often it read
//! the other's codes at each layout. A phone that hears nothing sweeps the
//! layouts; one that hears an echo stays on the layout the peer reads most.
//!
//! A pure state machine: every call takes the time, nothing here reads a
//! clock or draws anything. Layouts are ids; the app layer maps them to
//! placements.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::training_header::{
    ECHO_SLOTS, LAYOUT_COUNT, LayoutReads, MAX_READ_COUNT, TrainingHeader,
};

/// How long a read of a peer frame counts toward the echo.
pub const READ_WINDOW: Duration = Duration::from_secs(2);
/// How long after the peer's last echo this phone keeps its layout. Longer
/// than one sweep, so a peer that hears us once per sweep keeps us settled.
pub const ECHO_FRESH: Duration = Duration::from_secs(2);
/// Dwell of a frame shown while sweeping.
pub const SWEEP_DWELL_MS: u32 = 100;
/// Dwell while sweeping for a peer that reported reading fast. Both rig
/// phones held a 60 ms tick without skipping (journal K4, 2026-10-02).
pub const FAST_SWEEP_DWELL_MS: u32 = 60;
/// Reads per [`READ_WINDOW`] from which a peer counts as reading fast: more
/// than one read per `SWEEP_DWELL_MS` frame.
pub const FAST_READER_READS: u8 = 30;

/// Reads per [`READ_WINDOW`] the peer must report before chunks step up to
/// QR version 7, and to version 8. Deliberately high: the count measures
/// reads of version-6 frames, and a denser frame may not read at all in a
/// direction that reads those (journal T11). The rig decides whether
/// stepping up pays (design D4).
pub const STEP_UP_TO_V7_READS: u8 = 20;
pub const STEP_UP_TO_V8_READS: u8 = 40;

/// Plaintext bytes per DATA chunk for a peer that reports `peer_reads`
/// reads in its window.
///
/// A full chunk's frame is `50 + 1.5 × (bytes + 28) + ack` characters. The
/// sizes leave room for a 5-character ACK field (a peer sending up to 24
/// chunks) and six more characters under what QR versions 5, 7 and 8 hold
/// at level L (154, 224, 279): an encoder that splits the frame into
/// segments spends a few bits per extra segment, and 2 characters of room
/// was not always enough (a 152-char frame drawn at 41 modules once in a
/// test run).
pub fn chunk_bytes_for(peer_reads: u8) -> usize {
    if peer_reads >= STEP_UP_TO_V8_READS {
        78
    } else if peer_reads >= STEP_UP_TO_V7_READS {
        50
    } else {
        34
    }
}

/// Upper bound on reads kept, whatever the clock does.
const MAX_READS_KEPT: usize = 256;

/// Chooses the layout of each frame and builds its header.
#[derive(Debug, Clone)]
pub struct LinkTrainer {
    /// Peer frames read inside the window, oldest first.
    reads: VecDeque<(Instant, u8)>,
    /// The layout the peer last reported reading most, and when.
    settled: Option<(u8, Instant)>,
    /// Layout of the frame shown last.
    shown: Option<u8>,
    /// Where a running sweep continues; `None` when the next sweep starts
    /// over at its start layout.
    sweep_next: Option<u8>,
    start_layout: u8,
    held: bool,
    peer_reads_fast: bool,
}

impl LinkTrainer {
    /// A trainer whose first sweep starts at `start_layout`, the layout that
    /// last worked on this device. An id outside the set starts at full
    /// size.
    pub fn starting_at(start_layout: u8) -> Self {
        Self {
            reads: VecDeque::new(),
            settled: None,
            shown: None,
            sweep_next: None,
            start_layout: if start_layout < LAYOUT_COUNT {
                start_layout
            } else {
                0
            },
            held: false,
            peer_reads_fast: false,
        }
    }

    /// Record a frame read from the peer: where it was drawn, and what it
    /// says about how well the peer reads us.
    pub fn note_peer_frame(&mut self, header: &TrainingHeader, now: Instant) {
        self.reads.push_back((now, header.layout()));
        self.forget_old_reads(now);
        // A camera can deliver the same frame many times within the window;
        // the counts saturate long before this.
        if self.reads.len() > MAX_READS_KEPT {
            self.reads.pop_front();
        }
        self.peer_reads_fast = header.total_reads() >= FAST_READER_READS;
        if self.held {
            return;
        }
        if let Some(best) = self.best_echoed_layout(header) {
            self.settled = Some((best, now));
            self.sweep_next = None;
        }
    }

    /// The header for the next frame; its layout is where to draw it.
    pub fn next_frame(&mut self, now: Instant) -> TrainingHeader {
        self.forget_old_reads(now);
        let layout = self.next_layout(now);
        self.shown = Some(layout);
        let (echo, total) = self.echo(now);
        TrainingHeader::new(layout, &echo, total)
            .expect("layout, echo and total are built within their ranges")
    }

    /// The header of a frame drawn where the last one was, without moving
    /// the sweep on. For a frame shown outside the display cycle.
    pub fn current_frame(&self, now: Instant) -> TrainingHeader {
        let layout = self
            .shown
            .or(self.fresh_layout(now))
            .unwrap_or(self.sweep_start());
        let (echo, total) = self.echo(now);
        TrainingHeader::new(layout, &echo, total)
            .expect("layout, echo and total are built within their ranges")
    }

    /// Whether the next frame is part of a sweep: the peer has not echoed
    /// recently and the layout is not held.
    pub fn is_sweeping(&self, now: Instant) -> bool {
        !self.held && self.fresh_layout(now).is_none()
    }

    /// Whether a peer frame was read inside the window.
    pub fn is_reading_peer(&self, now: Instant) -> bool {
        self.reads
            .iter()
            .any(|(at, _)| now.saturating_duration_since(*at) < READ_WINDOW)
    }

    /// Dwell for a frame shown while sweeping.
    pub fn sweep_dwell_ms(&self) -> u32 {
        if self.peer_reads_fast {
            FAST_SWEEP_DWELL_MS
        } else {
            SWEEP_DWELL_MS
        }
    }

    /// Stop adapting: every later frame is drawn where the last one was.
    /// Used once the exchange is saved, when no echo can arrive any more.
    pub fn hold(&mut self) {
        self.held = true;
    }

    /// The layout the peer last echoed, for the next session's first sweep.
    pub fn last_good_layout(&self) -> Option<u8> {
        self.settled.map(|(layout, _)| layout)
    }

    fn next_layout(&mut self, now: Instant) -> u8 {
        if self.held {
            return self.shown.unwrap_or(self.sweep_start());
        }
        if let Some(layout) = self.fresh_layout(now) {
            return layout;
        }
        let layout = self.sweep_next.unwrap_or(self.sweep_start());
        self.sweep_next = Some((layout + 1) % LAYOUT_COUNT);
        layout
    }

    fn fresh_layout(&self, now: Instant) -> Option<u8> {
        self.settled
            .filter(|(_, at)| now.saturating_duration_since(*at) < ECHO_FRESH)
            .map(|(layout, _)| layout)
    }

    fn sweep_start(&self) -> u8 {
        self.last_good_layout().unwrap_or(self.start_layout)
    }

    /// The layout to settle on from a peer's echo: the one it read most.
    /// The current layout wins a tie, then the larger (lower id) one.
    fn best_echoed_layout(&self, header: &TrainingHeader) -> Option<u8> {
        let current = self.settled.map(|(layout, _)| layout);
        header
            .echo()
            .max_by_key(|reads| {
                (
                    reads.count,
                    Some(reads.layout) == current,
                    std::cmp::Reverse(reads.layout),
                )
            })
            .map(|reads| reads.layout)
    }

    fn forget_old_reads(&mut self, now: Instant) {
        while self
            .reads
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) >= READ_WINDOW)
        {
            self.reads.pop_front();
        }
    }

    /// The peer layouts read most in the window, most read first, and the
    /// number of reads in the window; both saturate at one digit.
    fn echo(&self, now: Instant) -> (Vec<LayoutReads>, u8) {
        let in_window = |at: &Instant| now.saturating_duration_since(*at) < READ_WINDOW;
        let mut counts = [0usize; LAYOUT_COUNT as usize];
        let mut total = 0usize;
        for (_, layout) in self.reads.iter().filter(|(at, _)| in_window(at)) {
            counts[usize::from(*layout)] += 1;
            total += 1;
        }
        let saturate = |n: usize| u8::try_from(n).unwrap_or(u8::MAX).min(MAX_READ_COUNT);
        let mut read: Vec<LayoutReads> = (0..LAYOUT_COUNT)
            .filter(|layout| counts[usize::from(*layout)] > 0)
            .map(|layout| LayoutReads {
                layout,
                count: saturate(counts[usize::from(layout)]),
            })
            .collect();
        read.sort_by_key(|reads| {
            (
                std::cmp::Reverse(counts[usize::from(reads.layout)]),
                reads.layout,
            )
        });
        read.truncate(ECHO_SLOTS);
        (read, saturate(total))
    }
}
