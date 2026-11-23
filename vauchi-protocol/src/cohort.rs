// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The cohort actions inside an OHTTP request (ADR-081, proposed;
//! vauchi/private#1281). Bytes travel in standard base64. Provisional, like
//! the relay's actions they match: no deployment serves them yet.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CohortTier {
    Fast,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CohortClass {
    Notice,
    Content,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohortRegisterRequest {
    pub cohort: u32,
    pub device: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohortWriteRequest {
    pub cohort: u32,
    pub device: String,
    pub tier: CohortTier,
    pub class: CohortClass,
    pub slot: u16,
    pub record: String,
}

/// The batch of the period that holds `at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohortBatchRequest {
    pub cohort: u32,
    pub tier: CohortTier,
    pub class: CohortClass,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohortChunkRequest {
    pub cohort: u32,
    pub tier: CohortTier,
    pub class: CohortClass,
    pub period: u64,
    pub index: usize,
}

/// Any cohort action's answer: `status` is "ok" or "error", and the other
/// fields are those the action returns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CohortResponse {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub records: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunks: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk: Option<String>,
}

impl CohortResponse {
    pub fn is_ok(&self) -> bool {
        self.status == "ok"
    }
}
