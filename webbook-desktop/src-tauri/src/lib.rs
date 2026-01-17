//! WebBook Desktop Application
//!
//! Tauri-based desktop app for WebBook.

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
        .setup(|app| {
            // Resolve data directory
            let data_dir = dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("webbook");

            // Initialize app state
            let app_state = AppState::new(&data_dir)
                .expect("Failed to initialize app state");

            app.manage(Mutex::new(app_state));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::identity::has_identity,
            commands::identity::create_identity,
            commands::identity::get_identity_info,
            commands::card::get_card,
            commands::card::add_field,
            commands::card::remove_field,
            commands::contacts::list_contacts,
            commands::contacts::get_contact,
            commands::contacts::remove_contact,
            commands::exchange::generate_qr,
            commands::exchange::complete_exchange,
            commands::backup::export_backup,
            commands::backup::import_backup,
            commands::backup::check_password_strength,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
