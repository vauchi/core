// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::{Arc, mpsc};
use std::time::Duration;

use crate::event_path_support::{action, activate, first_input, row, validation_errors};
use vauchi_app::content::{
    ContentCache, ContentSource, ContentSourceHandle, ContentType, compute_checksum,
};
use vauchi_app::ui::{AppEngine, AppScreen, ContentRunOptions};
use vauchi_core::api::Vauchi;
use vauchi_core::{Event, FieldType, InputValue};

const CACHED_NETWORKS: &[u8] = br#"[{
    "id": "cachedverse", "name": "Cachedverse",
    "url": "https://cachedverse.example/{username}",
    "username": {"max_len": 5, "charset": "alnum_underscore", "strip_at": true}
}]"#;

struct NoNetwork(mpsc::Sender<()>);

impl ContentSource for NoNetwork {
    fn fetch(&self, _path: &str) -> Result<Vec<u8>, String> {
        self.0.send(()).expect("offline request observer");
        Err("network unreachable".into())
    }
}

// @scenario: contact_card_management :: Use cached social network config when offline
#[test]
fn offline_social_picker_uses_cached_network_and_saves_through_events() {
    let dir = tempfile::tempdir().unwrap();
    ContentCache::new(dir.path())
        .unwrap()
        .save_content(
            ContentType::Networks,
            "networks.json",
            CACHED_NETWORKS,
            &compute_checksum(CACHED_NETWORKS),
        )
        .unwrap();
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let (requested, requests) = mpsc::channel();
    engine.start_content_updates(
        dir.path(),
        ContentRunOptions {
            first_run_after: Duration::ZERO,
            source: Some(ContentSourceHandle(Arc::new(NoNetwork(requested)))),
            ..Default::default()
        },
    );
    requests
        .recv_timeout(Duration::from_secs(5))
        .expect("offline fetch attempted");

    engine.navigate_to(AppScreen::MyInfo);
    let my_card = engine.initial_commands().unwrap();
    let (surface_id, actions) = action(&my_card, "Actions").expect("Actions");
    let menu = activate(&mut engine, surface_id, &actions);
    let (surface_id, add) = action(&menu, "Add Entry").expect("Add Entry");
    let types = activate(&mut engine, surface_id, &add);
    let (surface_id, network) = row(&types, "Cachedverse");
    let form = activate(&mut engine, surface_id, &network);
    let (surface_id, binding_id) = first_input(&form);
    let typed = engine
        .dispatch(Event::ValueChanged {
            surface_id: surface_id.clone(),
            binding_id: binding_id.clone(),
            value: InputValue::Text("too_long".into()),
        })
        .unwrap();
    let (save_surface, save) = action(&typed, "Save")
        .or_else(|| action(&form, "Save"))
        .unwrap();
    let rejected = activate(&mut engine, save_surface, &save);
    assert_eq!(
        validation_errors(&rejected),
        vec!["Invalid format".to_string()]
    );
    assert_eq!(
        engine.vauchi().own_card().unwrap().unwrap().fields().len(),
        0
    );

    let typed = engine
        .dispatch(Event::ValueChanged {
            surface_id,
            binding_id,
            value: InputValue::Text("alice".into()),
        })
        .unwrap();
    let (surface_id, save) = action(&typed, "Save").unwrap();
    let saved = activate(&mut engine, surface_id, &save);
    assert_eq!(validation_errors(&saved), Vec::<String>::new());
    let card = engine.vauchi().own_card().unwrap().unwrap();
    assert_eq!(card.fields().len(), 1);
    let field = &card.fields()[0];
    assert_eq!(field.field_type(), FieldType::Social);
    assert_eq!(field.label(), "Cachedverse");
    assert_eq!(field.value(), "alice");
}
