//! Contact Actions Commands
//!
//! Commands for opening contact field values in external apps.

use serde::Serialize;
use vauchi_core::contact_card::{is_allowed_scheme, ContactAction, ContactField, FieldType};

/// Result of opening a contact field.
#[derive(Serialize)]
pub struct OpenResult {
    pub success: bool,
    pub action: String,
    pub uri: Option<String>,
    pub error: Option<String>,
}

/// Information about what action would be taken for a field.
#[derive(Serialize)]
pub struct ActionInfo {
    pub action_type: String,
    pub uri: Option<String>,
    pub can_open: bool,
}

/// Parse a field type string into FieldType enum.
fn parse_field_type(field_type: &str) -> FieldType {
    match field_type.to_lowercase().as_str() {
        "email" => FieldType::Email,
        "phone" => FieldType::Phone,
        "website" => FieldType::Website,
        "address" => FieldType::Address,
        "social" => FieldType::Social,
        _ => FieldType::Custom,
    }
}

/// Get information about what action would be taken for a contact field.
#[tauri::command]
pub fn get_field_action(field_type: String, label: String, value: String) -> ActionInfo {
    let ft = parse_field_type(&field_type);
    let field = ContactField::new(ft, &label, &value);

    let action = field.to_action();
    let uri = field.to_uri();

    let action_type = match &action {
        ContactAction::Call(_) => "call",
        ContactAction::SendSms(_) => "sms",
        ContactAction::SendEmail(_) => "email",
        ContactAction::OpenUrl(_) => "url",
        ContactAction::OpenMap(_) => "map",
        ContactAction::CopyToClipboard => "copy",
    };

    ActionInfo {
        action_type: action_type.to_string(),
        uri: uri.clone(),
        can_open: uri.is_some(),
    }
}

/// Open a contact field in the appropriate external application.
///
/// Uses vauchi-core's URI builder for security validation before opening.
#[tauri::command]
pub async fn open_contact_field(
    field_type: String,
    label: String,
    value: String,
) -> Result<OpenResult, String> {
    // Parse field type and create a ContactField
    let ft = parse_field_type(&field_type);
    let field = ContactField::new(ft, &label, &value);

    // Get the action and URI using vauchi-core's secure URI builder
    let action = field.to_action();
    let uri = field.to_uri();

    let action_type = match &action {
        ContactAction::Call(_) => "call",
        ContactAction::SendSms(_) => "sms",
        ContactAction::SendEmail(_) => "email",
        ContactAction::OpenUrl(_) => "url",
        ContactAction::OpenMap(_) => "map",
        ContactAction::CopyToClipboard => "copy",
    };

    // If no URI can be generated, return copy action
    let Some(uri_str) = uri else {
        return Ok(OpenResult {
            success: false,
            action: "copy".to_string(),
            uri: None,
            error: Some(
                "No action available for this field. Value copied to clipboard.".to_string(),
            ),
        });
    };

    // Extra security check: validate the URI scheme
    if let Some(scheme) = uri_str.split(':').next() {
        if !is_allowed_scheme(scheme) {
            let scheme_owned = scheme.to_string();
            return Ok(OpenResult {
                success: false,
                action: "blocked".to_string(),
                uri: Some(uri_str),
                error: Some(format!(
                    "URI scheme '{}' is not allowed for security reasons.",
                    scheme_owned
                )),
            });
        }
    }

    // Use the opener plugin to open the URI
    match tauri_plugin_opener::open_url(&uri_str, None::<&str>) {
        Ok(_) => Ok(OpenResult {
            success: true,
            action: action_type.to_string(),
            uri: Some(uri_str),
            error: None,
        }),
        Err(e) => Ok(OpenResult {
            success: false,
            action: action_type.to_string(),
            uri: Some(uri_str),
            error: Some(format!("Failed to open: {}", e)),
        }),
    }
}
