// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The relay's cohort actions (ADR-081, proposed; vauchi/private#1281),
//! over the OHTTP gateway only: a direct request would show the relay the
//! device's address with its slots, so there is no direct fallback.

use vauchi_protocol::cohort::{
    CohortBatchRequest, CohortChunkRequest, CohortRegisterRequest, CohortResponse,
    CohortWriteRequest,
};

use super::HttpTransport;
use crate::network::NetworkError;

impl HttpTransport {
    pub fn cohort_register(
        &self,
        request: &CohortRegisterRequest,
    ) -> Result<CohortResponse, NetworkError> {
        self.cohort_action("cohort_register", request)
    }

    pub fn cohort_write(
        &self,
        request: &CohortWriteRequest,
    ) -> Result<CohortResponse, NetworkError> {
        self.cohort_action("cohort_write", request)
    }

    pub fn cohort_batch(
        &self,
        request: &CohortBatchRequest,
    ) -> Result<CohortResponse, NetworkError> {
        self.cohort_action("cohort_batch", request)
    }

    pub fn cohort_chunk(
        &self,
        request: &CohortChunkRequest,
    ) -> Result<CohortResponse, NetworkError> {
        self.cohort_action("cohort_chunk", request)
    }

    fn cohort_action<Req: serde::Serialize>(
        &self,
        action: &str,
        request: &Req,
    ) -> Result<CohortResponse, NetworkError> {
        if self.ohttp_reader().is_none() {
            return Err(NetworkError::ConnectionFailed(
                "cohort actions travel over OHTTP only".into(),
            ));
        }
        self.post_via_ohttp_refreshing_stale_key(action, request)
    }
}
