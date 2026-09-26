// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertSpec {
    pub title: String,
    pub message: String,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToastSpec {
    pub message: String,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportFileSpec {
    pub suggested_name: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

/// A machine-readable document Core prepared for a shell that asked for one.
/// The shell writes `data` out unread; `schema` is a Core-owned, versioned
/// identifier (ADR-066 Amendment 2026-09-26 (b)).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSpec {
    pub media_type: String,
    pub schema: String,
    pub data: Vec<u8>,
}

/// How a one-shot invocation ended. Deliberately generic: shells map it to
/// their own status convention (the CLI to exit codes) without learning why.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationOutcome {
    Succeeded,
    Failed,
    InvalidInput,
    Unavailable,
    Denied,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NotificationUrgency {
    Default,
    High,
    Urgent,
}

#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationSpec {
    pub title: String,
    pub body: String,
    pub deep_link_uri: Option<String>,
    pub category_id: String,
    pub channel_id: String,
    pub urgency: NotificationUrgency,
    pub category_options: Vec<String>,
}
