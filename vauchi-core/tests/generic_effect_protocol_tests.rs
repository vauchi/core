// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{
    AlertSpec, AuthenticationRequirement, Command, ExportFileSpec, NotificationSpec,
    NotificationUrgency, ToastSpec,
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
