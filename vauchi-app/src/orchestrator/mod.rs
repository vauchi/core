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

/// Host-side rendezvous for local (non-relay) device linking, ADR-070.
pub mod local_rendezvous;

#[cfg(all(feature = "network-http", feature = "storage"))]
pub mod device_link_machine;

#[cfg(all(feature = "network-http", feature = "storage"))]
pub mod device_link_responder_machine;

/// Multi-stage face-to-face exchange machine (slice 32m Phase 1).
/// In-person, BLE-less — no network-http feature required.
pub mod multi_stage_machine;

/// BLE handshake machine (slice 32m Phase 2). Replaces the
/// `MobileBleExchangeSession` + `MobileBleDelegate` callback trait
/// from `vauchi-platform/src/mobile_ble.rs`. In-person; no
/// network-http feature required.
pub mod ble_handshake_machine;
