//! Sync Commands
//!
//! Handles synchronization with the relay server.

use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

use crate::state::AppState;

/// Result of a sync operation.
#[derive(Serialize)]
pub struct SyncResult {
    /// Number of contacts added from exchange messages.
    pub contacts_added: u32,
    /// Number of contact cards updated.
    pub cards_updated: u32,
    /// Number of outbound updates sent.
    pub updates_sent: u32,
    /// Whether sync completed successfully.
    pub success: bool,
    /// Error message if sync failed.
    pub error: Option<String>,
}

/// Sync status for display.
#[derive(Serialize)]
pub struct SyncStatus {
    /// Number of pending outbound updates.
    pub pending_updates: u32,
    /// Last sync timestamp (Unix seconds), if available.
    pub last_sync: Option<u64>,
    /// Whether currently syncing.
    pub is_syncing: bool,
}

/// Perform a sync with the relay server.
///
/// This sends pending updates to contacts and receives incoming updates.
#[tauri::command]
pub fn sync(state: State<'_, Mutex<AppState>>) -> Result<SyncResult, String> {
    let state = state.lock().unwrap();

    // Verify we have an identity
    if state.identity.is_none() {
        return Err("No identity found. Please create an identity first.".to_string());
    }

    // Count pending updates
    let contacts = state
        .storage
        .list_contacts()
        .map_err(|e| format!("Failed to list contacts: {:?}", e))?;

    let mut total_pending = 0u32;
    for contact in &contacts {
        let pending = state
            .storage
            .get_pending_updates(contact.id())
            .unwrap_or_default();
        total_pending += pending.len() as u32;
    }

    // TODO: Implement actual relay connection and sync
    // For now, return a placeholder result indicating sync is not yet implemented
    // The full implementation would:
    // 1. Connect to the relay WebSocket server
    // 2. Send pending card updates to each contact
    // 3. Receive incoming card updates from contacts
    // 4. Process incoming exchange messages
    // 5. Handle acknowledgments

    Ok(SyncResult {
        contacts_added: 0,
        cards_updated: 0,
        updates_sent: 0,
        success: true,
        error: Some("Sync with relay not yet implemented. Use mobile app for full sync.".to_string()),
    })
}

/// Get the current sync status.
#[tauri::command]
pub fn get_sync_status(state: State<'_, Mutex<AppState>>) -> Result<SyncStatus, String> {
    let state = state.lock().unwrap();

    if state.identity.is_none() {
        return Ok(SyncStatus {
            pending_updates: 0,
            last_sync: None,
            is_syncing: false,
        });
    }

    // Count pending updates across all contacts
    let contacts = state
        .storage
        .list_contacts()
        .map_err(|e| format!("Failed to list contacts: {:?}", e))?;

    let mut total_pending = 0u32;
    for contact in &contacts {
        let pending = state
            .storage
            .get_pending_updates(contact.id())
            .unwrap_or_default();
        total_pending += pending.len() as u32;
    }

    Ok(SyncStatus {
        pending_updates: total_pending,
        last_sync: None, // TODO: Store and retrieve last sync time
        is_syncing: false,
    })
}
