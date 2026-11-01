// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Exchange session metadata enums (transport, proximity, audio, origin).
//!
//! A neutral leaf module: shared by `exchange`, `contact`, `storage`, and
//! `api` without depending on any of them, so these enums never pull the
//! heavy `exchange` module into a lightweight consumer.

//!
//! The enums are contract types since ADR-078 (`vauchi_contract::exchange_types`);
//! this module re-exports them at their old path.

pub use vauchi_contract::exchange_types::*;
