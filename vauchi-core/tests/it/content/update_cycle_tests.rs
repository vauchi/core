// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! One content update cycle, as both binding layers run it: check, then
//! apply only when updates are available (vauchi/private#733 D10).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use tempfile::TempDir;
use vauchi_app::content::{
    ApplyResult, ContentConfig, ContentIndex, ContentManager, ContentManifest, ContentType,
    FileEntry, LocalesEntry, UpdateStatus, compute_checksum,
};
use vauchi_core::crypto::signing::SigningKeyPair;

const PORTUGUESE: &str = r#"{"_meta":{"locale":"pt","name":"Português","english_name":"Portuguese","is_rtl":false},"cycle.probe":"do CDN"}"#;

fn serve(responses: Vec<Vec<u8>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for body in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
    });
    format!("http://{address}")
}

fn manifest(signer: Option<&SigningKeyPair>) -> Vec<u8> {
    let files = HashMap::from([(
        "pt".to_string(),
        FileEntry {
            path: "pt.json".to_string(),
            checksum: compute_checksum(PORTUGUESE.as_bytes()),
            size_bytes: PORTUGUESE.len() as u64,
            name: None,
            english_name: None,
            is_rtl: None,
        },
    )]);
    let mut manifest = ContentManifest {
        schema_version: 1,
        generated_at: "2026-10-22T00:00:00Z".to_string(),
        base_url: "https://cdn.vauchi.app/v1/".to_string(),
        content: ContentIndex {
            locales: Some(LocalesEntry {
                version: "1.0.0".to_string(),
                path: "locales/".to_string(),
                files,
                min_app_version: "0.1.0".to_string(),
            }),
            ..ContentIndex::default()
        },
        signature: None,
    };
    if let Some(signer) = signer {
        let canonical = serde_json::to_vec(&serde_json::to_value(&manifest).unwrap()).unwrap();
        manifest.signature = Some(hex::encode(signer.sign(&canonical).as_bytes()));
    }
    serde_json::to_vec(&manifest).unwrap()
}

fn manager(temp: &TempDir, url: String, publisher: &SigningKeyPair) -> ContentManager {
    let config = ContentConfig {
        storage_path: temp.path().to_path_buf(),
        content_url: url,
        publisher_public_key: publisher.public_key(),
        ..Default::default()
    };
    ContentManager::new(config, vauchi_core::clock::SystemClock::shared()).unwrap()
}

// @internal
#[test]
fn a_cycle_applies_what_the_signed_manifest_offers() {
    let temp = TempDir::new().unwrap();
    let publisher = SigningKeyPair::generate();
    let signed = manifest(Some(&publisher));
    let url = serve(vec![signed.clone(), signed, PORTUGUESE.as_bytes().to_vec()]);

    let (status, apply) = manager(&temp, url, &publisher).run_update_cycle_blocking();

    assert!(
        matches!(&status, UpdateStatus::UpdatesAvailable(types) if types == &vec![ContentType::Locales]),
        "{status:?}"
    );
    assert!(
        matches!(&apply, Some(Ok(ApplyResult::Applied { applied, failed }))
            if applied == &vec![ContentType::Locales] && failed.is_empty()),
        "{apply:?}"
    );
    assert_eq!(
        vauchi_app::i18n::get_string(
            vauchi_app::i18n::Locale::from_code("pt").unwrap(),
            "cycle.probe"
        ),
        "do CDN"
    );
}

// @internal
#[test]
fn a_cycle_does_not_apply_after_a_failed_check() {
    let temp = TempDir::new().unwrap();
    let publisher = SigningKeyPair::generate();
    let url = serve(vec![manifest(None)]);

    let (status, apply) = manager(&temp, url, &publisher).run_update_cycle_blocking();

    assert!(matches!(status, UpdateStatus::CheckFailed(_)), "{status:?}");
    assert!(apply.is_none(), "{apply:?}");
}
