// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The deletion the user runs from Privacy asks the relay to purge what it
//! holds for the identity, before the app wipes itself (#711 step 4).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::storage::DeletionState;
use vauchi_core::{Vauchi, VauchiConfig};

/// A relay that answers every POST with `{"status":"ok"}` and reports the
/// path and body of each one.
fn recording_relay() -> (String, mpsc::Receiver<(String, String)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (sent, received) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut content_length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header.trim().is_empty() {
                    break;
                }
                if let Some((name, value)) = header.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    content_length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; content_length];
            reader.read_exact(&mut body).unwrap();
            let path = request_line
                .split_whitespace()
                .nth(1)
                .unwrap_or("")
                .to_string();
            let _ = sent.send((path, String::from_utf8_lossy(&body).into_owned()));
            let reply = br#"{"status":"ok","blobs_deleted":0}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                reply.len()
            );
            let _ = stream.write_all(reply);
        }
    });
    (url, received)
}

/// Runs a deletion scheduled with `erase_guardian_set` and returns the
/// purges the relay received and the identity's public key.
fn execute_scheduled_deletion(erase_guardian_set: bool) -> (Vec<serde_json::Value>, String) {
    let (relay_url, received) = recording_relay();
    let mut config = VauchiConfig::default().with_relay_url(&relay_url);
    config.ohttp.allow_direct = true;
    config.relay.connect_timeout_ms = 2_000;
    let dir = tempfile::tempdir().unwrap();
    config.storage_path = dir.path().join("deleting.db");
    let mut vauchi = Vauchi::new(config).unwrap();
    vauchi.create_identity("Alice").unwrap();
    let public_key = hex::encode(vauchi.identity().unwrap().signing_public_key());
    vauchi
        .storage()
        .consent()
        .save_deletion_state(&DeletionState::Scheduled {
            scheduled_at: 1,
            execute_at: 1,
            erase_guardian_set,
        })
        .unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Privacy);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "execute_deletion".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_execute".into(),
    });

    assert!(matches!(result, ActionResult::WipeComplete));
    let purges = received
        .try_iter()
        .filter(|(path, _)| path == "/v2/purge")
        .map(|(_, body)| serde_json::from_str(&body).unwrap())
        .collect();
    (purges, public_key)
}

// @scenario: privacy_compliance :: The relay purge carries my recovery choice
#[test]
fn executing_the_deletion_purges_the_relay_before_the_wipe() {
    let (purges, public_key) = execute_scheduled_deletion(false);

    assert_eq!(purges.len(), 1, "one purge reaches the relay");
    assert_eq!(purges[0]["public_key"], public_key);
    assert!(purges[0].get("erase_guardian_set").is_none());
}

// @scenario: privacy_compliance :: The relay purge carries my recovery choice
#[test]
fn a_deletion_scheduled_to_erase_everything_purges_the_guardian_set_too() {
    let (purges, _) = execute_scheduled_deletion(true);

    assert_eq!(purges.len(), 1);
    assert_eq!(purges[0]["erase_guardian_set"], true);
}
