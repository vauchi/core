// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Platform command/event protocol (ADR-031).
//!
//! Core emits [`Command`]s to tell frontends what platform actions to
//! perform: hardware (display QR, start BLE scan, emit audio challenge),
//! IO (file picker, image picker, share sheet), and screen presentation
//! (brightness, idle timer, orientation lock).
//!
//! Frontends report results back via [`Event`]s.
//!
//! Originally introduced under ADR-031 §Hardware as `Command` /
//! `Event` for peer-to-peer exchange transports. The
//! protocol grew past that scope (avatar editor, backup import, screen
//! presentation) — renamed 2026-05-04 to drop the misleading
//! domain prefix. See investigation
//! `_private/docs/investigations/2026-05-04-exchange-command-naming-categorization.md`.
//!
//! Three-axis boundary protocol:
//! - [`crate::ui::UserAction`] — user → core
//! - [`Command`] — core → platform
//! - [`Event`] — platform → core
//!
//! Decouples the protocol state machines from platform-specific APIs
//! and works over UniFFI / C ABI boundaries (all types are serializable).

//!
//! Since ADR-078 the types live in the contract crate (`vauchi-contract`,
//! vauchi/contract); this module re-exports them so every path under
//! `vauchi_core::platform` keeps working for Core's own modules and for
//! the repositories that depend on Core.

pub use vauchi_contract::*;
