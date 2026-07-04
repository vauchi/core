// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange-success ceremony (M2 S4, Phase 0 of
//! `designs/2026-06-06-exchange-ceremony-design.md`).
//!
//! One constructor, one fixed intent triple. Every transport's validated
//! success emits exactly this command — never on failure — and no emission
//! site conditions it on anything (auth mode included), so the ceremony is
//! byte-identical under duress (ADR-032 parity; the serialization is pinned
//! in `tests/it/ceremony_wiring_tests.rs`).

use vauchi_core::Command;
use vauchi_core::platform::{AnimationToken, HapticPattern, SoundToken};

/// The exchange-success ceremony command ("clinking glasses").
pub(crate) fn exchange_celebrate() -> Command {
    Command::Celebrate {
        haptic: HapticPattern::Success,
        sound: SoundToken::ExchangeChime,
        animation: AnimationToken::CardsMeet,
    }
}
