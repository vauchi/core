// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The published relay wire schema (private#119, spec decision 5): a JSON
//! Schema of every V2 request and response type, generated from the Rust
//! types with the parse-boundary bounds written in, and one example per
//! type that parses. `schemas/relay-wire-v2.schema.json` and
//! `schemas/relay-wire-v2-examples.json` are the committed copies; the
//! specification repository mirrors them and validates the examples
//! against the schema in its CI. Regenerate with
//! `VAUCHI_WRITE_WIRE_SCHEMA=1 cargo test -p vauchi-protocol --features
//! schema-gen --test it wire_schema`.

use schemars::r#gen::{SchemaGenerator, SchemaSettings};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use vauchi_protocol::v2::bounds::{FIELD_BOUNDS, FieldBound};
use vauchi_protocol::v2::*;

const SCHEMA: &str = "schemas/relay-wire-v2.schema.json";
const EXAMPLES: &str = "schemas/relay-wire-v2-examples.json";
const SCHEMA_ID: &str = "https://spec.vauchi.app/schemas/relay-wire-v2.schema.json";

fn hex64() -> String {
    "ab".repeat(32)
}

fn sig128() -> String {
    "cd".repeat(64)
}

/// Every wire type, so each has one example and one schema definition, and
/// so a bound can be proved against the parser by type name.
macro_rules! wire_types {
    ($($t:ident),* $(,)?) => {
        const TYPE_NAMES: &[&str] = &[$(stringify!($t)),*];

        fn add_every_type(generator: &mut SchemaGenerator) {
            $(generator.subschema_for::<$t>();)*
        }

        fn parses_as(type_name: &str, value: Value) -> Result<(), String> {
            match type_name {
                $(stringify!($t) => serde_json::from_value::<$t>(value)
                    .map(drop)
                    .map_err(|e| e.to_string()),)*
                other => panic!("no wire type named {other}"),
            }
        }
    };
}

wire_types!(
    V2SendRequest,
    V2FetchRequest,
    V2AckRequest,
    V2RegisterRequest,
    V2PurgeRequest,
    V2ExchangeOfferRequest,
    V2ExchangeClaimRequest,
    V2ExchangeCompleteRequest,
    FetchedBlob,
    V2RecoveryStoreRequest,
    V2RecoveryQueryRequest,
    V2RecoveryProof,
    V2GuardianEntry,
    V2GuardianStoreRequest,
    V2GuardianQueryRequest,
    V2GuardianDeleteRequest,
    V2Response,
);

fn examples() -> Map<String, Value> {
    let hex = hex64();
    let sig = sig128();
    let at = 1_761_955_200_u64;
    let blob = json!({
        "blob_id": hex,
        "ciphertext": "U2VhbGVkIGVudmVsb3Bl",
        "created_at": at,
        "mailbox_token": hex,
        "origin_hint": "b3JpZ2lu"
    });
    let proof = json!({
        "key_hash": hex,
        "proof_data": "cHJvb2Y=",
        "created_at": at,
        "expires_at": at + 30 * 86_400
    });
    let entry = json!({ "data": "Z3VhcmRpYW4=" });
    let examples = json!({
        "V2SendRequest": {
            "recipient_id": hex,
            "ciphertext": "U2VhbGVkIGVudmVsb3Bl",
            "origin_hint": "b3JpZ2lu"
        },
        "V2FetchRequest": { "mailbox_tokens": [hex, "ef".repeat(32)] },
        "V2AckRequest": { "recipient_id": hex, "blob_id": hex },
        "V2RegisterRequest": { "mailbox_tokens": [hex] },
        "V2PurgeRequest": {
            "public_key": hex,
            "purge_token": hex,
            "signature": sig,
            "timestamp": at,
            "erase_guardian_set": true
        },
        "V2ExchangeOfferRequest": { "payload": "b2ZmZXI=", "expires_secs": 600 },
        "V2ExchangeClaimRequest": { "code": "ABCD-EFGH", "response": "cmVzcG9uc2U=" },
        "V2ExchangeCompleteRequest": { "code": "ABCD-EFGH" },
        "FetchedBlob": blob,
        "V2RecoveryStoreRequest": {
            "key_hash": hex,
            "proof_data": "cHJvb2Y=",
            "owner_pk": hex,
            "timestamp": at,
            "signature": sig
        },
        "V2RecoveryQueryRequest": { "key_hashes": [hex] },
        "V2RecoveryProof": proof,
        "V2GuardianEntry": entry,
        "V2GuardianStoreRequest": {
            "guardian_hash": hex,
            "entries": [entry],
            "designator_pk": hex,
            "timestamp": at,
            "signature": sig
        },
        "V2GuardianQueryRequest": { "guardian_hash": hex },
        "V2GuardianDeleteRequest": {
            "guardian_hash": hex,
            "designator_pk": hex,
            "timestamp": at,
            "signature": sig
        },
        "V2Response": {
            "status": "ok",
            "blob_id": hex,
            "blobs": [blob],
            "truncated": false,
            "acknowledged": true,
            "code": "ABCD-EFGH",
            "payload": "b2ZmZXI=",
            "response": "cmVzcG9uc2U=",
            "blobs_deleted": 2,
            "proofs": [proof],
            "guardians": [entry]
        }
    });
    match examples {
        Value::Object(map) => map,
        _ => unreachable!(),
    }
}

/// The schema as the code generates it: schemars' shape of each type, with
/// the parse-boundary bounds written onto the fields they guard.
fn generated_schema() -> Value {
    let mut generator = SchemaGenerator::new(SchemaSettings::draft07());
    add_every_type(&mut generator);
    let mut definitions =
        serde_json::to_value(generator.take_definitions()).expect("definitions serialize");
    for bound in FIELD_BOUNDS {
        let property = definitions
            .get_mut(bound.type_name)
            .and_then(|t| t.get_mut("properties"))
            .and_then(|p| p.get_mut(bound.field))
            .unwrap_or_else(|| {
                panic!(
                    "{}.{} is not a schema property",
                    bound.type_name, bound.field
                )
            });
        if let Some(n) = bound.max_len {
            property["maxLength"] = json!(n);
        }
        if let Some(n) = bound.max_items {
            property["maxItems"] = json!(n);
        }
        if let Some(n) = bound.item_max_len {
            property["items"]["maxLength"] = json!(n);
        }
    }
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "$id": SCHEMA_ID,
        "title": "Vauchi relay wire, version 2",
        "description": "The JSON bodies of the relay's /v2 requests and responses, \
            generated from vauchi-protocol's types by its wire_schema_tests. \
            Length and item bounds are the parser's; a longer value is refused \
            before any handler runs. Fields not listed are accepted and ignored, \
            as the parser does. Charsets (hex, base64) are documented, not \
            enforced at the parse boundary.",
        "definitions": definitions
    })
}

fn pretty(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("json serializes");
    text.push('\n');
    text
}

fn with_field(example: &Value, field: &str, value: Value) -> Value {
    let mut copy = example.clone();
    copy[field] = value;
    copy
}

fn example_of(bound: &FieldBound) -> Value {
    examples()
        .remove(bound.type_name)
        .unwrap_or_else(|| panic!("no example for {}", bound.type_name))
}

/// One item of a bounded list, as long as the list allows.
fn list_item(bound: &FieldBound, example: &Value) -> Value {
    match bound.item_max_len {
        Some(n) => json!("a".repeat(n)),
        None => example[bound.field][0].clone(),
    }
}

// @internal
#[test]
fn every_published_bound_is_the_parsers_bound() {
    for bound in FIELD_BOUNDS {
        let example = example_of(bound);
        let name = format!("{}.{}", bound.type_name, bound.field);
        if let Some(n) = bound.max_len {
            let at = with_field(&example, bound.field, json!("a".repeat(n)));
            let over = with_field(&example, bound.field, json!("a".repeat(n + 1)));
            assert_eq!(
                parses_as(bound.type_name, at),
                Ok(()),
                "{name} at {n} bytes"
            );
            assert!(
                parses_as(bound.type_name, over).is_err(),
                "{name} at {} bytes",
                n + 1
            );
        }
        if let Some(n) = bound.max_items {
            let item = list_item(bound, &example);
            let at = with_field(&example, bound.field, Value::Array(vec![item.clone(); n]));
            let over = with_field(&example, bound.field, Value::Array(vec![item; n + 1]));
            assert_eq!(
                parses_as(bound.type_name, at),
                Ok(()),
                "{name} with {n} items"
            );
            assert!(
                parses_as(bound.type_name, over).is_err(),
                "{name} with {} items",
                n + 1
            );
        }
        if let Some(n) = bound.item_max_len {
            let over = with_field(&example, bound.field, json!(["a".repeat(n + 1)]));
            assert!(
                parses_as(bound.type_name, over).is_err(),
                "{name} item at {} bytes",
                n + 1
            );
        }
    }
}

// @internal
#[test]
fn every_wire_type_has_one_example_that_parses_and_one_definition() {
    let examples = examples();
    let schema = generated_schema();
    let defined: BTreeSet<&str> = schema["definitions"]
        .as_object()
        .expect("definitions object")
        .keys()
        .map(String::as_str)
        .collect();
    let exampled: BTreeSet<&str> = examples.keys().map(String::as_str).collect();
    let named: BTreeSet<&str> = TYPE_NAMES.iter().copied().collect();
    assert_eq!(
        defined, named,
        "every wire type is a definition, and nothing else is"
    );
    assert_eq!(exampled, named, "every wire type has one example");
    for (type_name, example) in &examples {
        assert_eq!(
            parses_as(type_name, example.clone()),
            Ok(()),
            "{type_name} example"
        );
    }
}

// @internal
#[test]
fn the_committed_schema_and_examples_are_what_the_code_generates() {
    let schema = pretty(&generated_schema());
    let examples = pretty(&Value::Object(examples()));
    if std::env::var_os("VAUCHI_WRITE_WIRE_SCHEMA").is_some() {
        std::fs::create_dir_all("schemas").expect("schemas dir");
        std::fs::write(SCHEMA, &schema).expect("write schema");
        std::fs::write(EXAMPLES, &examples).expect("write examples");
    }
    let committed_schema = std::fs::read_to_string(SCHEMA).expect("committed schema");
    let committed_examples = std::fs::read_to_string(EXAMPLES).expect("committed examples");
    assert_eq!(
        committed_schema, schema,
        "{SCHEMA} differs from the code; regenerate with VAUCHI_WRITE_WIRE_SCHEMA=1"
    );
    assert_eq!(
        committed_examples, examples,
        "{EXAMPLES} differs from the code; regenerate with VAUCHI_WRITE_WIRE_SCHEMA=1"
    );
}
