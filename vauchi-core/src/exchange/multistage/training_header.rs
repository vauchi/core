// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The link-training header every multi-stage frame carries.
//!
//! Two phones facing each other each draw their code at one of
//! [`LAYOUT_COUNT`] placements and learn from the other which placements its
//! camera reads. The header is how they tell each other: the placement this
//! frame is drawn at, and how often this phone read the peer's frames at the
//! placements it read most.
//!
//! Eight base45 digits, so the opening frame stays within QR version 6:
//! `<layout><l1><c1><l2><c2><l3><c3><total>`. An unused echo slot is
//! `<NO_LAYOUT_DIGIT>0`.

use super::base45;
use super::qr_codec::QrCodecError;

/// Number of placements a code can be drawn at.
pub const LAYOUT_COUNT: u8 = 14;
/// Echo slots in one header.
pub const ECHO_SLOTS: usize = 3;
/// Largest read count one base45 digit holds; counts saturate here.
pub const MAX_READ_COUNT: u8 = 44;
/// Encoded header length in characters.
pub const HEADER_LEN: usize = 2 + 2 * ECHO_SLOTS;

/// Digit value that marks an echo slot as unused.
const NO_LAYOUT_DIGIT: u8 = 44;

/// How often this phone read the peer's frames drawn at `layout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutReads {
    pub layout: u8,
    pub count: u8,
}

/// The training header of one frame. Built only through [`Self::new`] and
/// [`Self::parse`], so a held value is always within range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrainingHeader {
    layout: u8,
    echo: [Option<LayoutReads>; ECHO_SLOTS],
    total_reads: u8,
}

impl TrainingHeader {
    /// A header for a frame drawn at `layout`, echoing up to [`ECHO_SLOTS`]
    /// peer layouts and the number of peer frames read in the window.
    ///
    /// Rejects a layout outside the set, more echo entries than slots, a
    /// layout echoed twice, and a count of zero or above [`MAX_READ_COUNT`].
    pub fn new(layout: u8, echo: &[LayoutReads], total_reads: u8) -> Result<Self, QrCodecError> {
        if layout >= LAYOUT_COUNT || echo.len() > ECHO_SLOTS || total_reads > MAX_READ_COUNT {
            return Err(QrCodecError::InvalidHeader);
        }
        let mut slots = [None; ECHO_SLOTS];
        for (slot, reads) in slots.iter_mut().zip(echo) {
            let in_range =
                reads.layout < LAYOUT_COUNT && (1..=MAX_READ_COUNT).contains(&reads.count);
            if !in_range {
                return Err(QrCodecError::InvalidHeader);
            }
            *slot = Some(*reads);
        }
        let header = Self {
            layout,
            echo: slots,
            total_reads,
        };
        if header.echoes_a_layout_twice() {
            return Err(QrCodecError::InvalidHeader);
        }
        Ok(header)
    }

    /// The layout this frame is drawn at.
    pub fn layout(&self) -> u8 {
        self.layout
    }

    /// The peer layouts the sender read most, with their counts.
    pub fn echo(&self) -> impl Iterator<Item = LayoutReads> + '_ {
        self.echo.iter().flatten().copied()
    }

    /// Peer frames the sender read in its window, saturating at
    /// [`MAX_READ_COUNT`].
    pub fn total_reads(&self) -> u8 {
        self.total_reads
    }

    fn echoes_a_layout_twice(&self) -> bool {
        self.echo().enumerate().any(|(i, reads)| {
            self.echo()
                .skip(i + 1)
                .any(|other| other.layout == reads.layout)
        })
    }

    /// Encode as [`HEADER_LEN`] base45 digits.
    pub fn encode(&self) -> String {
        let mut digits = Vec::with_capacity(HEADER_LEN);
        digits.push(self.layout);
        for slot in self.echo {
            match slot {
                Some(reads) => digits.extend([reads.layout, reads.count]),
                None => digits.extend([NO_LAYOUT_DIGIT, 0]),
            }
        }
        digits.push(self.total_reads);
        digits.into_iter().map(base45::digit).collect()
    }

    /// Parse [`HEADER_LEN`] base45 digits, as received from a camera.
    pub fn parse(encoded: &str) -> Result<Self, QrCodecError> {
        let digits = encoded
            .bytes()
            .map(base45::digit_value)
            .collect::<Result<Vec<u8>, _>>()
            .map_err(|_| QrCodecError::InvalidHeader)?;
        let [layout, slots @ .., total_reads] = digits.as_slice() else {
            return Err(QrCodecError::InvalidHeader);
        };
        if slots.len() != 2 * ECHO_SLOTS {
            return Err(QrCodecError::InvalidHeader);
        }
        let mut echo = Vec::with_capacity(ECHO_SLOTS);
        let mut seen_unused = false;
        for pair in slots.chunks_exact(2) {
            match (pair[0], pair[1]) {
                (NO_LAYOUT_DIGIT, 0) => seen_unused = true,
                // Used slots come first: one encoding per header.
                _ if seen_unused => return Err(QrCodecError::InvalidHeader),
                (layout, count) => echo.push(LayoutReads { layout, count }),
            }
        }
        Self::new(*layout, &echo, *total_reads)
    }
}
