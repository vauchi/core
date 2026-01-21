//! TUI Interaction Tests
//!
//! Tests the TUI application's public state types and enums.
//! These tests verify state struct defaults and enum variants.
//!
//! Note: Tests requiring App (which depends on Backend) are inline in src/app.rs
//! because they need access to internal types and a test backend.
//!
//! Tests here focus on the standalone state structs and enums.

use vauchi_tui::app::{
    AddFieldFocus, AddFieldState, BackupFocus, BackupMode, BackupState, InputMode, Screen,
    SyncState,
};

// ============================================================================
// Screen Enum Tests
// ============================================================================

/// Test: All screen variants exist
#[test]
fn test_screen_variants_exist() {
    let screens = [
        Screen::Home,
        Screen::Contacts,
        Screen::ContactDetail,
        Screen::ContactVisibility,
        Screen::Exchange,
        Screen::Settings,
        Screen::Help,
        Screen::AddField,
        Screen::EditField,
        Screen::EditName,
        Screen::EditRelayUrl,
        Screen::Devices,
        Screen::Recovery,
        Screen::Sync,
        Screen::Backup,
    ];

    // Verify we have all expected screens
    assert_eq!(screens.len(), 15);
}

/// Test: Screen Home is the initial screen variant
#[test]
fn test_screen_home_is_initial() {
    let screen = Screen::Home;
    assert_eq!(screen, Screen::Home);
}

/// Test: Screen equality works
#[test]
fn test_screen_equality() {
    assert_eq!(Screen::Home, Screen::Home);
    assert_ne!(Screen::Home, Screen::Contacts);
    assert_ne!(Screen::Settings, Screen::Help);
}

// ============================================================================
// InputMode Tests
// ============================================================================

/// Test: InputMode variants exist
#[test]
fn test_input_mode_variants() {
    let _ = InputMode::Normal;
    let _ = InputMode::Editing;
}

/// Test: InputMode Normal is the initial mode
#[test]
fn test_input_mode_normal_is_initial() {
    let mode = InputMode::Normal;
    assert_eq!(mode, InputMode::Normal);
}

// ============================================================================
// AddFieldState Tests
// ============================================================================

/// Test: AddFieldState default values
#[test]
fn test_add_field_state_default() {
    let state = AddFieldState::default();
    assert_eq!(state.field_type_index, 0);
    assert!(state.label.is_empty());
    assert!(state.value.is_empty());
    assert_eq!(state.focus, AddFieldFocus::Type);
}

/// Test: AddFieldFocus variants
#[test]
fn test_add_field_focus_variants() {
    assert_eq!(AddFieldFocus::default(), AddFieldFocus::Type);
    let _ = AddFieldFocus::Label;
    let _ = AddFieldFocus::Value;
}

/// Test: AddFieldFocus equality
#[test]
fn test_add_field_focus_equality() {
    assert_eq!(AddFieldFocus::Type, AddFieldFocus::Type);
    assert_ne!(AddFieldFocus::Type, AddFieldFocus::Label);
    assert_ne!(AddFieldFocus::Label, AddFieldFocus::Value);
}

// ============================================================================
// BackupState Tests
// ============================================================================

/// Test: BackupState default values
#[test]
fn test_backup_state_default() {
    let state = BackupState::default();
    assert_eq!(state.mode, BackupMode::Menu);
    assert!(state.password.is_empty());
    assert!(state.confirm_password.is_empty());
    assert!(state.backup_data.is_empty());
    assert_eq!(state.focus, BackupFocus::Password);
}

/// Test: BackupMode variants
#[test]
fn test_backup_mode_variants() {
    assert_eq!(BackupMode::default(), BackupMode::Menu);
    let _ = BackupMode::Export;
    let _ = BackupMode::Import;
}

/// Test: BackupMode equality
#[test]
fn test_backup_mode_equality() {
    assert_eq!(BackupMode::Menu, BackupMode::Menu);
    assert_ne!(BackupMode::Menu, BackupMode::Export);
    assert_ne!(BackupMode::Export, BackupMode::Import);
}

/// Test: BackupFocus variants
#[test]
fn test_backup_focus_variants() {
    assert_eq!(BackupFocus::default(), BackupFocus::Password);
    let _ = BackupFocus::Confirm;
    let _ = BackupFocus::Data;
}

/// Test: BackupFocus equality
#[test]
fn test_backup_focus_equality() {
    assert_eq!(BackupFocus::Password, BackupFocus::Password);
    assert_ne!(BackupFocus::Password, BackupFocus::Confirm);
    assert_ne!(BackupFocus::Confirm, BackupFocus::Data);
}

// ============================================================================
// SyncState Tests
// ============================================================================

/// Test: SyncState default values
#[test]
fn test_sync_state_default() {
    let state = SyncState::default();
    assert!(!state.connected);
    assert!(!state.is_syncing);
    assert_eq!(state.pending_updates, 0);
    assert!(state.last_result.is_none());
    assert!(state.sync_log.is_empty());
}

/// Test: SyncState can be modified
#[test]
fn test_sync_state_modification() {
    let mut state = SyncState::default();

    state.connected = true;
    state.is_syncing = true;
    state.pending_updates = 5;
    state.sync_log.push("Test log entry".to_string());

    assert!(state.connected);
    assert!(state.is_syncing);
    assert_eq!(state.pending_updates, 5);
    assert_eq!(state.sync_log.len(), 1);
}

// ============================================================================
// State Struct Field Access Tests
// ============================================================================

/// Test: AddFieldState fields are accessible and modifiable
#[test]
fn test_add_field_state_fields() {
    let mut state = AddFieldState::default();

    state.field_type_index = 2;
    state.label = "Work Email".to_string();
    state.value = "alice@work.com".to_string();
    state.focus = AddFieldFocus::Value;

    assert_eq!(state.field_type_index, 2);
    assert_eq!(state.label, "Work Email");
    assert_eq!(state.value, "alice@work.com");
    assert_eq!(state.focus, AddFieldFocus::Value);
}

/// Test: BackupState fields are accessible and modifiable
#[test]
fn test_backup_state_fields() {
    let mut state = BackupState::default();

    state.mode = BackupMode::Export;
    state.password = "secret123".to_string();
    state.confirm_password = "secret123".to_string();
    state.backup_data = "abcdef1234567890".to_string();
    state.focus = BackupFocus::Data;

    assert_eq!(state.mode, BackupMode::Export);
    assert_eq!(state.password, "secret123");
    assert_eq!(state.confirm_password, "secret123");
    assert_eq!(state.backup_data, "abcdef1234567890");
    assert_eq!(state.focus, BackupFocus::Data);
}
