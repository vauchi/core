// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The activity log writer lives in Core, beside the store it writes and the
//! profile engines whose events it records (ADR-080).

pub use vauchi_core::activity_log::RETENTION_SECS;
pub use vauchi_core::api::ActivityLogWriter;
