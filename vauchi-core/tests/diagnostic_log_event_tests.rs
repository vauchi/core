// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::diagnostic::log_event::{LogEvent, LogEventKind};

#[test]
fn log_event_serializes_to_json() {
    let event = LogEvent {
        timestamp_ms: 1709712000000,
        device_model: "Pixel 7".into(),
        kind: LogEventKind::DecodeSuccess {
            latency_ms: 12.5,
            frame_index: 42,
        },
    };
    let json = serde_json::to_string(&event).expect("should serialize");
    assert!(json.contains("\"kind\":\"decode_success\""));
    assert!(json.contains("\"latency_ms\":12.5"));
    assert!(json.contains("\"frame_index\":42"));
}

#[test]
fn log_event_roundtrips_all_variants() {
    let events = vec![
        LogEventKind::DecodeSuccess {
            latency_ms: 5.0,
            frame_index: 1,
        },
        LogEventKind::DecodeFailure {
            reason: "no_qr".into(),
            frame_index: 2,
        },
        LogEventKind::CameraConfigApplied {
            config_id: 10,
            iso: 200,
            ev: -1,
            fps: 30,
        },
        LogEventKind::CameraConfigFailed {
            config_id: 11,
            reason: "unsupported".into(),
        },
        LogEventKind::ThermalState {
            state: "nominal".into(),
            temp_c: 35.2,
        },
        LogEventKind::SweepStarted { total_configs: 100 },
        LogEventKind::SweepPhaseComplete {
            phase: 1,
            top_configs: vec![5, 12, 3],
        },
        LogEventKind::SweepComplete {
            best_config_id: 5,
            best_score: 0.95,
        },
        LogEventKind::SnapshotSaved {
            frame_index: 7,
            path: "snap.jpg".into(),
        },
    ];

    for kind in events {
        let event = LogEvent {
            timestamp_ms: 0,
            device_model: "Test".into(),
            kind,
        };
        let json = serde_json::to_string(&event).expect("serialize");
        let back: LogEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.device_model, "Test");
        assert_eq!(back.timestamp_ms, 0);
    }
}

#[test]
fn log_event_parses_jsonl_stream() {
    // With #[serde(tag = "kind")] on LogEventKind, the kind field in LogEvent
    // is serialized as a nested object containing "kind":"VariantName" plus fields.
    let lines = vec![
        r#"{"timestamp_ms":1000,"device_model":"Pixel","kind":{"kind":"decode_success","latency_ms":5.0,"frame_index":1}}"#,
        r#"{"timestamp_ms":2000,"device_model":"Pixel","kind":{"kind":"thermal_state","state":"nominal","temp_c":36.0}}"#,
    ];
    for line in lines {
        let event: LogEvent = serde_json::from_str(line).expect("should parse JSONL line");
        assert_eq!(event.device_model, "Pixel");
    }
}

#[test]
fn log_event_decode_success_has_expected_fields() {
    let event = LogEvent {
        timestamp_ms: 500,
        device_model: "iPhone 15".into(),
        kind: LogEventKind::DecodeSuccess {
            latency_ms: 0.0,
            frame_index: 0,
        },
    };
    let json = serde_json::to_string(&event).expect("serialize");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse as Value");

    assert_eq!(parsed["timestamp_ms"], 500);
    assert_eq!(parsed["device_model"], "iPhone 15");
    assert_eq!(parsed["kind"]["kind"], "decode_success");
    assert_eq!(parsed["kind"]["latency_ms"], 0.0);
    assert_eq!(parsed["kind"]["frame_index"], 0);
}

#[test]
fn log_event_sweep_phase_complete_preserves_vec() {
    let event = LogEvent {
        timestamp_ms: 100,
        device_model: "Test".into(),
        kind: LogEventKind::SweepPhaseComplete {
            phase: 2,
            top_configs: vec![10, 20, 30],
        },
    };
    let json = serde_json::to_string(&event).expect("serialize");
    let back: LogEvent = serde_json::from_str(&json).expect("deserialize");

    match back.kind {
        LogEventKind::SweepPhaseComplete { phase, top_configs } => {
            assert_eq!(phase, 2);
            assert_eq!(top_configs, vec![10, 20, 30]);
        }
        other => panic!("expected SweepPhaseComplete, got {:?}", other),
    }
}

#[test]
fn log_event_rejects_invalid_json() {
    let bad_inputs = vec![
        r#"{}"#,
        r#"{"timestamp_ms":0}"#,
        r#"{"timestamp_ms":0,"device_model":"X","kind":{}}"#,
        r#"{"timestamp_ms":0,"device_model":"X","kind":{"kind":"non_existent"}}"#,
    ];
    for input in bad_inputs {
        let result = serde_json::from_str::<LogEvent>(input);
        assert!(
            result.is_err(),
            "expected error for input: {}, got: {:?}",
            input,
            result.unwrap()
        );
    }
}
