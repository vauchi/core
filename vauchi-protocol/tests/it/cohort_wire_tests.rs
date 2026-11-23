// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The cohort actions' wire types (ADR-081, proposed; vauchi/private#1281):
//! the JSON the relay's cohort actions parse (relay!623).

use serde_json::json;
use vauchi_protocol::cohort::{
    CohortBatchRequest, CohortChunkRequest, CohortClass, CohortRegisterRequest, CohortResponse,
    CohortTier, CohortWriteRequest,
};

// @internal
#[test]
fn a_write_request_is_the_json_the_relay_parses() {
    let request = CohortWriteRequest {
        cohort: 1,
        device: "AQ==".into(),
        tier: CohortTier::Fast,
        class: CohortClass::Notice,
        slot: 3,
        record: "Ag==".into(),
    };
    assert_eq!(
        serde_json::to_value(&request).expect("serializes"),
        json!({ "cohort": 1, "device": "AQ==", "tier": "fast", "class": "notice", "slot": 3, "record": "Ag==" })
    );
    let batch = CohortBatchRequest {
        cohort: 1,
        tier: CohortTier::Background,
        class: CohortClass::Content,
        at: 7,
    };
    assert_eq!(
        serde_json::to_value(&batch).expect("serializes"),
        json!({ "cohort": 1, "tier": "background", "class": "content", "at": 7 })
    );
    let chunk = CohortChunkRequest {
        cohort: 1,
        tier: CohortTier::Fast,
        class: CohortClass::Notice,
        period: 9,
        index: 2,
    };
    assert_eq!(
        serde_json::to_value(&chunk).expect("serializes"),
        json!({ "cohort": 1, "tier": "fast", "class": "notice", "period": 9, "index": 2 })
    );
    let register = CohortRegisterRequest {
        cohort: 1,
        device: "AQ==".into(),
    };
    assert_eq!(
        serde_json::to_value(&register).expect("serializes"),
        json!({ "cohort": 1, "device": "AQ==" })
    );
}

// @internal
#[test]
fn a_request_with_an_unknown_field_or_tier_is_refused() {
    let extra = json!({ "cohort": 1, "device": "AQ==", "tier": "fast", "class": "notice", "slot": 3, "record": "Ag==", "sender": "x" });
    assert!(serde_json::from_value::<CohortWriteRequest>(extra).is_err());
    let tier = json!({ "cohort": 1, "tier": "instant", "class": "notice", "at": 7 });
    assert!(serde_json::from_value::<CohortBatchRequest>(tier).is_err());
}

// @internal
#[test]
fn the_relays_answers_parse() {
    let written: CohortResponse =
        serde_json::from_value(json!({ "status": "ok", "written": "replaced" })).expect("parses");
    assert_eq!(written.written.as_deref(), Some("replaced"));
    let batch: CohortResponse =
        serde_json::from_value(json!({ "status": "ok", "period": 5, "records": 1, "chunks": 1 }))
            .expect("parses");
    assert_eq!(
        (batch.period, batch.records, batch.chunks),
        (Some(5), Some(1), Some(1))
    );
    let refused: CohortResponse =
        serde_json::from_value(json!({ "status": "error", "error": "not_published" }))
            .expect("parses");
    assert_eq!(refused.error.as_deref(), Some("not_published"));
    assert!(!refused.is_ok());
    assert!(written.is_ok());
}
