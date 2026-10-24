// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A bounds-checked cursor for length-prefixed binary formats.

/// Reads fields front to back, refusing any read past the end. Offsets use
/// checked arithmetic, so a lying `u32` length cannot wrap a 32-bit `usize`
/// into an in-bounds slice (vauchi/private#286). Each read returns `None`
/// on failure; callers map it to their own error.
pub(crate) struct ByteReader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> ByteReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    pub(crate) fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(len)?;
        let bytes = self.data.get(self.at..end)?;
        self.at = end;
        Some(bytes)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    pub(crate) fn u32_le(&mut self) -> Option<u32> {
        self.array::<4>().map(u32::from_le_bytes)
    }

    /// A `u32` length prefix followed by that many bytes.
    pub(crate) fn prefixed(&mut self) -> Option<&'a [u8]> {
        let len = usize::try_from(self.u32_le()?).ok()?;
        self.take(len)
    }

    /// Every byte has been read.
    pub(crate) fn is_at_end(&self) -> bool {
        self.at == self.data.len()
    }
}
