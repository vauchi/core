//! Vauchi Desktop Application
//!
//! Tauri-based desktop app for Vauchi.

mod commands;
mod state;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::Manager;

use state::AppState;

/// Initialize and run the Tauri application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Resolve data directory
            let data_dir = dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("vauchi");

            // Initialize app state
            let app_state = AppState::new(&data_dir).expect("Failed to initialize app state");

            app.manage(Mutex::new(app_state));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::identity::has_identity,
            commands::identity::create_identity,
            commands::identity::get_identity_info,
            commands::identity::update_display_name,
            commands::card::get_card,
            commands::card::add_field,
            commands::card::remove_field,
            commands::card::update_field,
            commands::contacts::list_contacts,
            commands::contacts::get_contact,
            commands::contacts::remove_contact,
            commands::contacts::get_contact_fingerprint,
            commands::contacts::verify_contact,
            commands::exchange::generate_qr,
            commands::exchange::complete_exchange,
            commands::backup::export_backup,
            commands::backup::import_backup,
            commands::backup::check_password_strength,
            commands::visibility::get_visibility_rules,
            commands::visibility::set_field_visibility,
            commands::visibility::get_contacts_for_visibility,
            commands::visibility::get_field_viewers,
            commands::labels::list_labels,
            commands::labels::create_label,
            commands::labels::get_label,
            commands::labels::rename_label,
            commands::labels::delete_label,
            commands::labels::add_contact_to_label,
            commands::labels::remove_contact_from_label,
            commands::labels::get_labels_for_contact,
            commands::labels::set_label_field_visibility,
            commands::labels::set_contact_field_override,
            commands::labels::remove_contact_field_override,
            commands::labels::get_suggested_labels,
            commands::devices::list_devices,
            commands::devices::get_current_device,
            commands::devices::generate_device_link,
            commands::devices::join_device,
            commands::devices::finish_join_device,
            commands::devices::complete_device_link,
            commands::devices::revoke_device,
            commands::recovery::get_recovery_settings,
            commands::recovery::create_recovery_claim,
            commands::recovery::create_recovery_voucher,
            commands::recovery::check_recovery_claim,
            commands::recovery::parse_recovery_claim,
            commands::actions::open_contact_field,
            commands::actions::get_field_action,
            commands::sync::sync,
            commands::sync::get_sync_status,
            commands::sync::get_relay_url,
            commands::sync::set_relay_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
