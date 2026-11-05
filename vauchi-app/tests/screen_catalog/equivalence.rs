// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Full catalog comparison with explicit exceptions for seeded entropy (#1087).

use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use vauchi_app::ui::ScreenCatalogFixture;
use vauchi_core::exchange::ExchangeQR;
use vauchi_core::exchange::link_mode::parse_link_url;
use vauchi_core::exchange::multistage::qr_codec::{StageQr, parse_frame};
use vauchi_core::recovery::RecoveryClaim;

pub fn comparable(catalog: &ScreenCatalogFixture) -> Value {
    let mut value = serde_json::to_value(catalog).expect("catalog serializes");
    let mut fields = BTreeMap::new();
    for screen in value["screens"].as_array_mut().expect("screens") {
        let code_id = screen["code_id"].as_str().expect("code id").to_owned();
        for command in screen["commands"].as_array_mut().expect("commands") {
            normalize_command(&code_id, command);
            if let Some(surface) = command.pointer_mut("/ReplaceSurface/surface") {
                normalize_nodes(&code_id, &mut surface["nodes"], &mut fields);
            }
        }
    }
    value
}

fn normalize_command(screen: &str, command: &mut Value) {
    if screen == "direct_transport"
        && let Some(payload) = command.pointer_mut("/DirectSend/payload")
    {
        let bytes: Vec<u8> = serde_json::from_value(payload.clone()).expect("typed direct payload");
        let encoded = std::str::from_utf8(&bytes).expect("encoded exchange payload");
        let qr = ExchangeQR::from_data_string(encoded).expect("valid direct exchange payload");
        assert!(qr.verify_signature(), "direct payload signature verifies");
        let decoded = STANDARD.decode(encoded).expect("base64 direct payload");
        *payload = json!({"version": decoded[4], "name": qr.display_name(),
            "relay_url": qr.relay_url(), "bytes": bytes.len()});
    }
    let payload = match screen {
        "ble_exchange-magic" | "ble_exchange-bump" | "ble_exchange-shake" => {
            command.pointer_mut("/BleStartAdvertising/payload")
        }
        _ => None,
    };
    // Seeded ephemeral keys/nonces change; retain byte count and all other fields.
    if let Some(payload) = payload {
        let len = payload.as_array().expect("typed byte payload").len();
        *payload = json!({"seeded_bytes": len});
    }
    if screen == "multi_stage_exchange-glance"
        && let Some(wakeup) = command.get_mut("ScheduleWakeup")
    {
        // The initial frame deadline is measured after real crypto work (#1087).
        let remaining = wakeup["earliest_millis"].as_u64().expect("frame deadline");
        let budget = wakeup["earliest_secs"].as_u64().expect("seconds deadline") * 1000;
        assert!(
            (1..=budget).contains(&remaining),
            "initial frame deadline outside budget"
        );
        assert_eq!(wakeup["delay_millis"], json!(remaining));
        wakeup["earliest_millis"] = json!("remaining initial frame time");
        wakeup["delay_millis"] = json!("remaining initial frame time");
    }
}

fn normalize_nodes(screen: &str, nodes: &mut Value, fields: &mut BTreeMap<String, usize>) {
    for node in nodes.as_array_mut().expect("nodes") {
        for kind in ["List", "Group", "Text"] {
            if screen == "contact_detail"
                && let Some(id) = node.pointer_mut(&format!("/{kind}/id"))
            {
                normalize_field_id(id, fields);
            }
        }
        if let Some(text) = node.get_mut("Text") {
            normalize_text(screen, text);
        }
        if screen == "multi_stage_exchange-glance"
            && node.pointer("/Qr/id").and_then(Value::as_str) == Some("surface.1.own_qr")
        {
            for payload in node["Qr"]["payloads"].as_array_mut().expect("QR payloads") {
                let frame =
                    parse_frame(payload.as_str().expect("QR string")).expect("valid QR frame");
                let StageQr::Init {
                    display_name,
                    relay_url,
                    ..
                } = frame.stage
                else {
                    panic!("initial Glance screen must carry an INIT frame");
                };
                *payload = json!({"header": format!("{:?}", frame.header),
                    "stage": "Init", "display_name": display_name, "relay_url": relay_url});
            }
        }
        if let Some(children) = node.pointer_mut("/Group/children") {
            normalize_nodes(screen, children, fields);
        }
    }
}

fn normalize_field_id(id: &mut Value, fields: &mut BTreeMap<String, usize>) {
    let original = id.as_str().expect("node id");
    for prefix in ["field_", "field_note:"] {
        if let Some(suffix) = original.strip_prefix(prefix)
            && suffix.len() == 16
            && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            let next = fields.len();
            let index = fields.entry(suffix.to_owned()).or_insert(next);
            *id = json!(format!("{prefix}seeded-{index}"));
            return;
        }
    }
}

fn normalize_text(screen: &str, text: &mut Value) {
    if screen == "recovery-hand_back_pending" && text["id"] == "claim_data" {
        let encoded = text["content"].as_str().expect("claim string");
        let bytes = STANDARD.decode(encoded).expect("base64 claim");
        let claim = RecoveryClaim::from_bytes(&bytes).expect("valid recovery claim");
        assert_eq!(
            STANDARD.encode(claim.to_bytes()),
            encoded,
            "canonical claim encoding"
        );
        assert_ne!(
            claim.old_pk(),
            claim.new_pk(),
            "recovery uses a temporary key"
        );
        assert_eq!(text["content"], text["accessibility"]["label"]);
        let normalized = json!({"version": bytes[0], "bytes": bytes.len(),
            "old_pk": "seeded target public key", "new_pk": "seeded temporary public key",
            "timestamp": claim.timestamp()});
        text["content"] = normalized.clone();
        text["accessibility"]["label"] = normalized;
    }
    if screen == "verify_fingerprint"
        && matches!(
            text["id"].as_str(),
            Some("their_fingerprint" | "our_fingerprint")
        )
    {
        let content = text["content"].as_str().expect("fingerprint");
        let words: Vec<_> = content.split(' ').collect();
        assert_eq!(words.len(), 16, "fingerprint length");
        assert!(
            words
                .iter()
                .all(|word| word.len() == 4 && word.bytes().all(|byte| byte.is_ascii_hexdigit())),
            "fingerprint format"
        );
        assert_eq!(text["content"], text["accessibility"]["label"]);
        text["content"] = json!("seeded fingerprint");
        text["accessibility"]["label"] = json!("seeded fingerprint");
    }
    if screen == "link_exchange" && text["id"] == "link_url" {
        let url = text["content"].as_str().expect("link URL");
        let parsed = parse_link_url(url).expect("valid link exchange URL");
        let parameters: Vec<_> = url
            .split('&')
            .map(|parameter| {
                if let Some((key, _)) = parameter.rsplit_once("pk=") {
                    format!("{key}pk=<seeded>")
                } else if parameter.starts_with("n=") {
                    "n=<seeded>".to_owned()
                } else {
                    parameter.to_owned()
                }
            })
            .collect();
        text["content"] = json!({"url": parameters.join("&"), "sender_name": parsed.sender_name});
    }
}
