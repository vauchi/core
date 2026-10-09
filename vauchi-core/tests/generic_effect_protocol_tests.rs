// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{
    AlertSpec, AuthenticationRequirement, Command, DocumentSpec, ExportFileSpec, InvocationOutcome,
    NotificationSpec, NotificationUrgency, ToastSpec,
};

// @internal
#[test]
fn generic_shell_effects_round_trip_without_domain_results() {
    let commands = vec![
        Command::PresentAlert {
            alert: AlertSpec {
                title: "Unable to continue".into(),
                message: "Try again later.".into(),
            },
        },
        Command::ShowToast {
            toast: ToastSpec {
                message: "Saved".into(),
            },
        },
        Command::OpenExternalUrl {
            url: "https://example.test/help".into(),
        },
        Command::ExportFile {
            file: ExportFileSpec {
                suggested_name: "export.json".into(),
                mime_type: "application/json".into(),
                data: b"{}".to_vec(),
            },
        },
        Command::PerformNativeBack,
        Command::ResetApplication,
        Command::RequestBiometricUnlock,
        Command::ForgetStoredSecret {
            handle: "legacy-storage-key".into(),
        },
    ];

    let encoded = serde_json::to_vec(&commands).expect("serialize generic effects");
    let decoded: Vec<Command> = serde_json::from_slice(&encoded).expect("decode generic effects");

    assert_eq!(decoded, commands);
}

// @internal
#[test]
fn notification_command_contains_only_prepared_os_presentation() {
    let command = Command::PostNotification {
        notification: NotificationSpec {
            title: "Card updated".into(),
            body: "Ada updated their card.".into(),
            deep_link_uri: Some("vauchi://contact/opaque".into()),
            category_id: "card-events".into(),
            channel_id: "updates".into(),
            urgency: NotificationUrgency::Default,
            category_options: vec!["hidden_preview".into()],
        },
    };

    let encoded = serde_json::to_vec(&command).expect("serialize notification");
    let decoded: Command = serde_json::from_slice(&encoded).expect("decode notification");

    assert_eq!(decoded, command);
}

// @internal
#[test]
fn authentication_requirement_is_a_generic_core_command() {
    let command = Command::SetAuthenticationRequirement {
        requirement: AuthenticationRequirement::AppPassword,
    };

    let encoded = serde_json::to_value(&command).expect("serialize authentication command");

    assert_eq!(
        encoded["SetAuthenticationRequirement"]["requirement"],
        "app_password"
    );
}

// @internal
#[test]
fn emitted_document_is_opaque_prepared_bytes_with_a_core_owned_schema() {
    let command = Command::EmitDocument {
        document: DocumentSpec {
            media_type: "application/json".into(),
            schema: "vauchi.contacts.v1".into(),
            data: b"[]".to_vec(),
        },
    };

    let encoded = serde_json::to_value(&command).expect("serialize document command");
    let decoded: Command = serde_json::from_value(encoded.clone()).expect("decode document");

    assert_eq!(
        encoded["EmitDocument"]["document"]["schema"],
        "vauchi.contacts.v1"
    );
    assert_eq!(
        encoded["EmitDocument"]["document"]["media_type"],
        "application/json"
    );
    assert_eq!(decoded, command);
    assert_eq!(command.variant_name(), "EmitDocument");
}

// @internal
#[test]
fn invocation_outcomes_are_a_closed_generic_set_on_the_wire() {
    let cases = [
        (InvocationOutcome::Succeeded, "succeeded"),
        (InvocationOutcome::Failed, "failed"),
        (InvocationOutcome::InvalidInput, "invalid_input"),
        (InvocationOutcome::Unavailable, "unavailable"),
        (InvocationOutcome::Denied, "denied"),
    ];

    for (outcome, wire) in cases {
        let command = Command::FinishInvocation { outcome };
        let encoded = serde_json::to_value(&command).expect("serialize outcome");

        assert_eq!(encoded["FinishInvocation"]["outcome"], wire);
        assert_eq!(
            serde_json::from_value::<Command>(encoded).expect("decode outcome"),
            command
        );
        assert_eq!(command.variant_name(), "FinishInvocation");
    }
    assert!(
        serde_json::from_value::<Command>(serde_json::json!({
            "FinishInvocation": { "outcome": "partially_succeeded" }
        }))
        .is_err(),
        "an unknown outcome must fail closed"
    );
}
