// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Platform-level entry into the engine-owned link-mode responder
//! (ADR-049). Onboarding → `UserAction::LinkOpened { uri }` → grant must
//! navigate to `link_responder_waiting`, and — now that core drives the
//! relay escrow round-trip itself (`AppEngine::advance_link_responder_session`),
//! since no frontend ever executed the `RelayEscrow*` commands — the grant
//! envelope must carry **no** escrow commands.
//!
//! The full deposit → gate-poll → ready → retrieve → terminal behaviour is
//! covered end-to-end against a mock relay in
//! `vauchi-core/tests/it/link_responder_poll_tests.rs`; the responder state
//! machine's event handling is covered in
//! `vauchi-core/tests/it/link_responder_tests.rs`. Those replace the four
//! prior envelope-/hand-fed-event tests, whose contract (the frontend
//! executing escrow commands and reporting events) was the dead path this
//! ADR retires.

use vauchi_platform::{PlatformAppEngine, PlatformAppEngineTestHelpers};

fn create_engine() -> (std::sync::Arc<PlatformAppEngine>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("create engine");
    (engine, dir)
}

/// Drive through the full onboarding flow via the canonical envelope.
///
/// Every step reads the Core-minted interaction and binding ids from the
/// current command batch — exactly what a real shell renders — and
/// dispatches generic events back. No retired action/screen seams.
fn drive_onboarding(engine: &PlatformAppEngine) {
    fn primary_interaction(batch: &serde_json::Value) -> (String, String) {
        let bar = batch["commands"]
            .as_array()
            .and_then(|commands| commands.iter().find_map(|c| c.get("SetContextBar")))
            .expect("command batch must carry a context bar");
        (
            bar["surface_id"]
                .as_str()
                .expect("bar surface id")
                .to_owned(),
            bar["bar"]["primary"]["interaction_id"]
                .as_str()
                .expect("primary interaction id")
                .to_owned(),
        )
    }

    fn dispatch_primary(
        engine: &PlatformAppEngine,
        batch: &serde_json::Value,
    ) -> serde_json::Value {
        let (surface_id, interaction_id) = primary_interaction(batch);
        let event = serde_json::json!({
            "ActionActivated": { "surface_id": surface_id, "interaction_id": interaction_id }
        });
        serde_json::from_str(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch primary activation"),
        )
        .expect("parse command batch")
    }

    fn find_input(nodes: &[serde_json::Value]) -> Option<&serde_json::Value> {
        nodes.iter().find_map(|node| {
            if let Some(input) = node.get("Input") {
                Some(input)
            } else {
                node["Group"]["children"]
                    .as_array()
                    .and_then(|children| find_input(children))
            }
        })
    }

    fn set_text_input(
        engine: &PlatformAppEngine,
        batch: &serde_json::Value,
        text: &str,
    ) -> serde_json::Value {
        let (surface_id, nodes) = batch["commands"]
            .as_array()
            .and_then(|commands| {
                commands.iter().find_map(|c| {
                    let surface = &c["ReplaceSurface"]["surface"];
                    surface
                        .is_object()
                        .then(|| (surface["surface_id"].clone(), surface["nodes"].clone()))
                })
            })
            .expect("command batch must replace a surface");
        let nodes: Vec<serde_json::Value> =
            serde_json::from_value(nodes).expect("surface nodes array");
        let input = find_input(&nodes).expect("surface must carry a text input");
        let event = serde_json::json!({
            "ValueChanged": {
                "surface_id": surface_id,
                "binding_id": input["binding_id"],
                "value": { "text": text },
            }
        });
        serde_json::from_str(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch text input"),
        )
        .expect("parse command batch")
    }

    let mut batch: serde_json::Value = serde_json::from_str(
        &engine
            .initial_commands_json()
            .expect("initial onboarding commands"),
    )
    .expect("parse initial batch");

    batch = dispatch_primary(engine, &batch); // identity_check → default_name
    batch = set_text_input(engine, &batch, "Bob"); // enter display name
    batch = dispatch_primary(engine, &batch); // default_name → groups_setup
    batch = dispatch_primary(engine, &batch); // groups_setup → contact_info
    batch = dispatch_primary(engine, &batch); // contact_info → what_next
    let _ = dispatch_primary(engine, &batch); // what_next → complete → home
}

fn fresh_link_url() -> String {
    let (init, _) = vauchi_core::exchange::link_mode::initiator_generate();
    init.url
}

/// Titles of every surface in the re-composed command batch (main and
/// secondary pane alike), read through the canonical path. The batch is
/// what a shell renders, so title membership is the honest observable
/// for flow state — the retired granular screen_id was Core-internal
/// vocabulary.
fn surface_titles(engine: &PlatformAppEngine) -> Vec<String> {
    let json = engine
        .initial_commands_json()
        .expect("initial_commands_json");
    let v: serde_json::Value = serde_json::from_str(&json).expect("parse commands json");
    v["commands"]
        .as_array()
        .map(|commands| {
            commands
                .iter()
                .filter_map(|c| {
                    c["ReplaceSurface"]["surface"]["title"]
                        .as_str()
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn grant_navigates_to_responder_with_no_frontend_escrow_commands() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let link_url = fresh_link_url();
    let consent_batch: serde_json::Value = serde_json::from_str(
        &engine
            .dispatch_json(serde_json::json!({"DeepLinkOpened": {"uri": link_url}}).to_string())
            .expect("DeepLinkOpened routes exchange URI to consent"),
    )
    .expect("consent envelope");
    assert!(
        surface_titles(&engine).contains(&"Exchange Request".to_string()),
        "LinkOpened must present the consent surface"
    );

    let (consent_surface, grant_id) = {
        let bar = consent_batch["commands"]
            .as_array()
            .and_then(|commands| commands.iter().find_map(|c| c.get("SetContextBar")))
            .expect("consent batch must carry a context bar");
        (
            bar["surface_id"]
                .as_str()
                .expect("bar surface id")
                .to_owned(),
            bar["bar"]["primary"]["interaction_id"]
                .as_str()
                .expect("grant interaction id")
                .to_owned(),
        )
    };
    let grant_json = engine
        .dispatch_json(
            serde_json::json!({
                "ActionActivated": { "surface_id": consent_surface, "interaction_id": grant_id }
            })
            .to_string(),
        )
        .expect("grant activation");
    assert!(
        surface_titles(&engine).contains(&"Waiting for Response".to_string()),
        "grant must route to the responder waiting screen — action returned {grant_json}",
    );

    // ADR-049: the responder's escrow deposits stay in the engine-owned
    // machine and are driven by `advance_link_responder_session`; they must
    // NOT be surfaced to the frontend command envelope (the dead path).
    let value: serde_json::Value =
        serde_json::from_str(&grant_json).expect("envelope is valid JSON");
    let commands = value["commands"]
        .as_array()
        .expect("envelope carries a commands array");
    let escrow_commands: Vec<&serde_json::Value> = commands
        .iter()
        .filter(|command| {
            command.get("RelayEscrowDeposit").is_some()
                || command.get("RelayEscrowCheck").is_some()
                || command.get("RelayEscrowRetrieve").is_some()
        })
        .collect();
    assert!(
        escrow_commands.is_empty(),
        "escrow is core-driven (ADR-049); the grant envelope must carry no \
         RelayEscrow* commands, got: {grant_json}",
    );
}
