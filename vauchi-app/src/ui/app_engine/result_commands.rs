// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{
    AlertSpec, Command, ExportFileSpec, NotificationSpec, NotificationUrgency, ToastSpec,
};

use crate::notification_types::NotificationPriority;
use crate::ui::ActionResult;

pub(super) fn append_result_commands(
    result: ActionResult,
    commands: &mut Vec<Command>,
) -> Result<(), &'static str> {
    match result {
        ActionResult::UpdateScreen(_)
        | ActionResult::NavigateTo(_)
        | ActionResult::Complete
        | ActionResult::OnboardingComplete { .. } => {}
        ActionResult::PerformNativeBack => commands.push(Command::PerformNativeBack),
        ActionResult::OpenUrl { url } => commands.push(Command::OpenExternalUrl { url }),
        ActionResult::ShowAlert { title, message }
        | ActionResult::ShowInfoOverlay {
            title,
            body: message,
        } => commands.push(Command::PresentAlert {
            alert: AlertSpec { title, message },
        }),
        ActionResult::ShowToast { message, .. } => commands.push(Command::ShowToast {
            toast: ToastSpec { message },
        }),
        ActionResult::RequestCamera => commands.push(Command::QrRequestScan),
        ActionResult::BackupExportComplete { data } => commands.push(Command::ExportFile {
            file: ExportFileSpec {
                suggested_name: "vauchi-backup.vauchi".into(),
                mime_type: "application/octet-stream".into(),
                data: data.into_bytes(),
            },
        }),
        ActionResult::GdprExportComplete { json } => commands.push(Command::ExportFile {
            file: ExportFileSpec {
                suggested_name: "vauchi-data-export.json".into(),
                mime_type: "application/json".into(),
                data: json.into_bytes(),
            },
        }),
        ActionResult::WipeComplete => commands.push(Command::ResetApplication),
        ActionResult::Commands {
            commands: result_commands,
        } => commands.extend(result_commands),
        ActionResult::Notify { notifications } => {
            commands.extend(notifications.into_iter().map(|notification| {
                Command::PostNotification {
                    notification: NotificationSpec {
                        title: notification.title,
                        body: notification.body,
                        deep_link_uri: notification.deep_link_uri,
                        category_id: notification.os_category_id,
                        channel_id: notification.os_channel_id,
                        urgency: match notification.priority {
                            NotificationPriority::Default => NotificationUrgency::Default,
                            NotificationPriority::High => NotificationUrgency::High,
                            NotificationPriority::Urgent => NotificationUrgency::Urgent,
                        },
                        category_options: notification.os_category_options,
                    },
                }
            }));
        }
        ActionResult::ValidationError { .. } => return Err("ValidationError"),
        ActionResult::CompleteWith { .. } => return Err("CompleteWith"),
        ActionResult::StartDeviceLink { .. } => return Err("StartDeviceLink"),
        ActionResult::OpenContact { .. } => return Err("OpenContact"),
        ActionResult::ContactAction { .. } => return Err("ContactAction"),
        ActionResult::EditContact { .. } => return Err("EditContact"),
        ActionResult::OpenEntryDetail { .. } => return Err("OpenEntryDetail"),
        ActionResult::DeviceLinkJoinStart { .. } => return Err("DeviceLinkJoinStart"),
        ActionResult::PreviewAs { .. } => return Err("PreviewAs"),
        ActionResult::ShowContactPicker => return Err("ShowContactPicker"),
        ActionResult::VerifyFingerprint { .. } => return Err("VerifyFingerprint"),
        ActionResult::ShowFormDialog { .. } => return Err("ShowFormDialog"),
        ActionResult::SetGroupFieldVisibility { .. } => {
            return Err("SetGroupFieldVisibility");
        }
        ActionResult::RetryFailedDeliveries { .. } => return Err("RetryFailedDeliveries"),
        ActionResult::StartMultiStageExchange { .. } => return Err("StartMultiStageExchange"),
        ActionResult::StartLinkExchange => return Err("StartLinkExchange"),
        ActionResult::StartBleExchange { .. } => return Err("StartBleExchange"),
        ActionResult::StartNfcExchange => return Err("StartNfcExchange"),
        ActionResult::StartDirectTransport => return Err("StartDirectTransport"),
        ActionResult::DeviceLinkConfirmManual { .. } => return Err("DeviceLinkConfirmManual"),
        ActionResult::DeviceLinkDeny => return Err("DeviceLinkDeny"),
        ActionResult::DeviceLinkRetry => return Err("DeviceLinkRetry"),
        ActionResult::BiometricUnlockOutcome { .. } => return Err("BiometricUnlockOutcome"),
    }
    Ok(())
}

// INLINE_TEST_REQUIRED: this private adapter proves that internal result types
// are converted before a command batch reaches any shell.
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn alert_and_native_back_become_generic_commands() {
        let mut commands = Vec::new();
        append_result_commands(
            ActionResult::ShowAlert {
                title: "Error".into(),
                message: "Try again".into(),
            },
            &mut commands,
        )
        .expect("alert maps to a generic command");
        append_result_commands(ActionResult::PerformNativeBack, &mut commands)
            .expect("native back maps to a generic command");

        assert!(matches!(commands[0], Command::PresentAlert { .. }));
        assert_eq!(commands[1], Command::PerformNativeBack);
    }

    // @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
    #[test]
    fn unresolved_internal_result_fails_closed() {
        let error = append_result_commands(
            ActionResult::ValidationError {
                component_id: "name".into(),
                message: "Required".into(),
            },
            &mut Vec::new(),
        )
        .expect_err("an internal result must not disappear at the reducer boundary");

        assert_eq!(error, "ValidationError");
    }
}
