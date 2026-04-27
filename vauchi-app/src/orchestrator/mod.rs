// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Cross-binding orchestrators.
//!
//! Plain-Rust state-machine + cycle-thread modules consumed by both
//! the UniFFI binding crate (`vauchi-platform`) and the C-ABI binding
//! crate (`vauchi-cabi`). Each binding provides its own listener
//! adapter; the orchestrator owns the protocol clock and cycle-thread
//! lifecycle, in line with ADR-021 (Humble UI).

#[cfg(feature = "network-http")]
pub mod device_link_relay;
