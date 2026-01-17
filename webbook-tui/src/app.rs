//! Application State

use crate::backend::Backend;

/// Current screen in the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Home screen with contact card
    Home,
    /// Contact list
    Contacts,
    /// Contact detail view
    ContactDetail,
    /// Contact visibility settings
    ContactVisibility,
    /// QR exchange screen
    Exchange,
    /// Settings screen
    Settings,
    /// Help screen
    Help,
    /// Add field dialog
    AddField,
    /// Device management screen
    Devices,
    /// Recovery screen
    Recovery,
    /// Sync status screen
    Sync,
    /// Backup/restore screen
    Backup,
}

/// Input mode for text entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Normal navigation mode
    Normal,
    /// Editing text
    Editing,
}

/// Application state.
#[allow(dead_code)]
pub struct App {
    /// WebBook backend
    pub backend: Backend,
    /// Current screen
    pub screen: Screen,
    /// Input mode
    pub input_mode: InputMode,
    /// Whether the app should quit (for future use)
    pub should_quit: bool,
    /// Status message
    pub status_message: Option<String>,
    /// Selected contact index (for contacts list)
    pub selected_contact: usize,
    /// Selected field index (for card fields)
    pub selected_field: usize,
    /// Selected field index in contact detail view
    pub selected_contact_field: usize,
    /// Text input buffer
    pub input_buffer: String,
    /// Add field state
    pub add_field_state: AddFieldState,
    /// Visibility screen state
    pub visibility_state: VisibilityState,
    /// Backup screen state
    pub backup_state: BackupState,
    /// Selected device index
    pub selected_device: usize,
    /// Contact search query
    pub contact_search_query: String,
    /// Contact search mode active
    pub contact_search_mode: bool,
}

/// State for the add field dialog.
#[derive(Debug, Default)]
pub struct AddFieldState {
    pub field_type_index: usize,
    pub label: String,
    pub value: String,
    pub focus: AddFieldFocus,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AddFieldFocus {
    #[default]
    Type,
    Label,
    Value,
}

/// State for the visibility screen.
#[derive(Debug, Default)]
pub struct VisibilityState {
    pub contact_id: Option<String>,
    pub selected_field: usize,
}

/// State for the backup screen.
#[derive(Debug, Default)]
pub struct BackupState {
    pub mode: BackupMode,
    pub password: String,
    pub confirm_password: String,
    pub backup_data: String,
    pub focus: BackupFocus,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BackupMode {
    #[default]
    Menu,
    Export,
    Import,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BackupFocus {
    #[default]
    Password,
    Confirm,
    Data,
}

impl App {
    /// Create a new application.
    pub fn new(backend: Backend) -> Self {
        let _has_identity = backend.has_identity();

        App {
            backend,
            screen: Screen::Home,
            input_mode: InputMode::Normal,
            should_quit: false,
            status_message: None,
            selected_contact: 0,
            selected_field: 0,
            selected_contact_field: 0,
            input_buffer: String::new(),
            add_field_state: AddFieldState::default(),
            visibility_state: VisibilityState::default(),
            backup_state: BackupState::default(),
            selected_device: 0,
            contact_search_query: String::new(),
            contact_search_mode: false,
        }
    }

    /// Set a status message.
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    /// Clear the status message.
    #[allow(dead_code)]
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// Navigate to a screen.
    pub fn goto(&mut self, screen: Screen) {
        self.screen = screen;
        self.input_mode = InputMode::Normal;
    }

    /// Go back to the previous screen.
    pub fn go_back(&mut self) {
        match self.screen {
            Screen::Contacts
            | Screen::Exchange
            | Screen::Settings
            | Screen::Help
            | Screen::Devices
            | Screen::Recovery
            | Screen::Sync
            | Screen::Backup => {
                self.screen = Screen::Home;
            }
            Screen::ContactDetail => {
                self.screen = Screen::Contacts;
            }
            Screen::ContactVisibility => {
                self.screen = Screen::ContactDetail;
                self.visibility_state = VisibilityState::default();
            }
            Screen::AddField => {
                self.screen = Screen::Home;
                self.add_field_state = AddFieldState::default();
            }
            _ => {}
        }
        self.input_mode = InputMode::Normal;
    }
}
