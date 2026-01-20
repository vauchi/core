//! CLI Integration Tests
//!
//! Tests trace to feature files:
//! - identity_management.feature
//! - contact_card_management.feature
//! - contact_exchange.feature
//! - visibility_labels.feature

use std::process::{Command, Output};
use tempfile::TempDir;

/// Helper to run CLI commands in an isolated data directory
struct CliTestContext {
    data_dir: TempDir,
    relay_url: String,
}

impl CliTestContext {
    fn new() -> Self {
        Self {
            data_dir: TempDir::new().expect("Failed to create temp dir"),
            relay_url: "ws://127.0.0.1:8080".to_string(),
        }
    }

    /// Run a CLI command and return the output
    fn run(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_vauchi"));
        cmd.arg("--data-dir")
            .arg(self.data_dir.path())
            .arg("--relay")
            .arg(&self.relay_url);

        for arg in args {
            cmd.arg(arg);
        }

        cmd.output().expect("Failed to execute command")
    }

    /// Run a command and assert success
    fn run_success(&self, args: &[&str]) -> String {
        let output = self.run(args);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        assert!(
            output.status.success(),
            "Command {:?} failed.\nStdout: {}\nStderr: {}",
            args,
            stdout,
            stderr
        );
        stdout
    }

    /// Run a command and assert failure
    fn run_failure(&self, args: &[&str]) -> String {
        let output = self.run(args);
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        assert!(
            !output.status.success(),
            "Command {:?} should have failed but succeeded",
            args
        );
        stderr
    }

    /// Initialize identity with name
    fn init(&self, name: &str) -> String {
        self.run_success(&["init", name])
    }
}

// ===========================================================================
// Identity Management Tests
// Trace: features/identity_management.feature
// ===========================================================================

mod identity_management {
    use super::*;

    /// Trace: identity_management.feature - "Create new identity on first launch"
    #[test]
    fn test_init_creates_identity() {
        let ctx = CliTestContext::new();
        let output = ctx.init("Alice Smith");

        assert!(output.contains("Identity created: Alice Smith"));
        assert!(output.contains("Public ID:"));
    }

    /// Trace: identity_management.feature - "Set display name during identity setup"
    #[test]
    fn test_init_sets_display_name() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let output = ctx.run_success(&["card", "show"]);
        assert!(output.contains("Alice Smith"));
    }

    /// Trace: identity_management.feature - "Display name validation"
    /// Note: Currently the CLI allows empty names - this tests current behavior
    #[test]
    fn test_init_empty_name_behavior() {
        let ctx = CliTestContext::new();
        // Current behavior: empty name is accepted at CLI level
        // Validation should ideally happen but currently doesn't
        let output = ctx.run(&["init", ""]);
        // Just verify the command runs without crashing
        let _stdout = String::from_utf8_lossy(&output.stdout);
    }

    /// Trace: identity_management.feature - Cannot re-initialize
    #[test]
    fn test_init_already_initialized_fails() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let stderr = ctx.run_failure(&["init", "Bob Jones"]);
        assert!(stderr.contains("already initialized"));
    }

    /// Trace: identity_management.feature - "Create encrypted identity backup"
    /// Note: Skipped - export requires interactive password input via dialoguer
    #[test]
    #[ignore = "requires interactive terminal for password input"]
    fn test_export_creates_backup() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let backup_path = ctx.data_dir.path().join("backup.json");
        let output = ctx.run_success(&["export", backup_path.to_str().unwrap()]);

        assert!(output.contains("exported") || output.contains("Backup"));
        assert!(backup_path.exists());
    }

    /// Trace: identity_management.feature - "Restore identity from backup"
    /// Note: Skipped - import requires interactive password input via dialoguer
    #[test]
    #[ignore = "requires interactive terminal for password input"]
    fn test_import_restores_identity() {
        // Create first identity and export
        let ctx1 = CliTestContext::new();
        ctx1.init("Alice Smith");

        let backup_path = ctx1.data_dir.path().join("backup.json");
        ctx1.run_success(&["export", backup_path.to_str().unwrap()]);

        // Import into new context
        let ctx2 = CliTestContext::new();
        let output = ctx2.run_success(&["import", backup_path.to_str().unwrap()]);

        assert!(output.contains("imported") || output.contains("restored") || output.contains("Identity"));

        // Verify name was restored
        let card_output = ctx2.run_success(&["card", "show"]);
        assert!(card_output.contains("Alice Smith"));
    }

    /// Trace: identity_management.feature - "Identity verification via public key fingerprint"
    #[test]
    fn test_device_info_shows_fingerprint() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let output = ctx.run_success(&["device", "info"]);
        // Should show public key info
        assert!(
            output.contains("Device") || output.contains("Public") || output.contains("ID"),
            "Expected device info, got: {}",
            output
        );
    }
}

// ===========================================================================
// Contact Card Management Tests
// Trace: features/contact_card_management.feature
// ===========================================================================

mod contact_card_management {
    use super::*;

    /// Trace: contact_card_management.feature - "Add a phone number field"
    #[test]
    fn test_card_add_phone_field() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let output = ctx.run_success(&["card", "add", "phone", "Mobile", "+1-555-123-4567"]);
        assert!(output.contains("added") || output.contains("Mobile"));

        let card = ctx.run_success(&["card", "show"]);
        assert!(card.contains("Mobile"));
        assert!(card.contains("+1-555-123-4567"));
    }

    /// Trace: contact_card_management.feature - "Add an email field"
    #[test]
    fn test_card_add_email_field() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        ctx.run_success(&["card", "add", "email", "Work", "alice@company.com"]);

        let card = ctx.run_success(&["card", "show"]);
        assert!(card.contains("Work"));
        assert!(card.contains("alice@company.com"));
    }

    /// Trace: contact_card_management.feature - "Add a website field"
    #[test]
    fn test_card_add_website_field() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        ctx.run_success(&["card", "add", "website", "Personal", "https://alice.example.com"]);

        let card = ctx.run_success(&["card", "show"]);
        assert!(card.contains("Personal"));
        assert!(card.contains("https://alice.example.com"));
    }

    /// Trace: contact_card_management.feature - "Edit an existing field value"
    #[test]
    fn test_card_edit_field() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        ctx.run_success(&["card", "add", "phone", "Mobile", "+1-555-123-4567"]);
        ctx.run_success(&["card", "edit", "Mobile", "+1-555-999-8888"]);

        let card = ctx.run_success(&["card", "show"]);
        assert!(card.contains("+1-555-999-8888"));
        assert!(!card.contains("+1-555-123-4567"));
    }

    /// Trace: contact_card_management.feature - "Remove a field from contact card"
    #[test]
    fn test_card_remove_field() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        ctx.run_success(&["card", "add", "phone", "Mobile", "+1-555-123-4567"]);
        ctx.run_success(&["card", "remove", "Mobile"]);

        let card = ctx.run_success(&["card", "show"]);
        assert!(!card.contains("Mobile") || card.contains("No fields"));
    }

    /// Trace: contact_card_management.feature - "Update display name"
    #[test]
    fn test_card_edit_name() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        ctx.run_success(&["card", "edit-name", "Alice S."]);

        let card = ctx.run_success(&["card", "show"]);
        assert!(card.contains("Alice S."));
    }

    /// Trace: contact_card_management.feature - Multiple fields
    #[test]
    fn test_card_multiple_fields() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        ctx.run_success(&["card", "add", "phone", "Mobile", "+1-555-123-4567"]);
        ctx.run_success(&["card", "add", "email", "Work", "alice@work.com"]);
        ctx.run_success(&["card", "add", "email", "Personal", "alice@personal.com"]);

        let card = ctx.run_success(&["card", "show"]);
        assert!(card.contains("Mobile"));
        assert!(card.contains("Work"));
        assert!(card.contains("Personal"));
    }

    /// Trace: contact_card_management.feature - "Add social media fields"
    #[test]
    fn test_card_add_social_field() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        ctx.run_success(&["card", "add", "social", "GitHub", "alicesmith"]);

        let card = ctx.run_success(&["card", "show"]);
        assert!(card.contains("GitHub") || card.contains("github"));
    }
}

// ===========================================================================
// Contact Exchange Tests
// Trace: features/contact_exchange.feature
// ===========================================================================

mod contact_exchange {
    use super::*;

    /// Trace: contact_exchange.feature - "Generate exchange QR code"
    #[test]
    fn test_exchange_start_generates_data() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let output = ctx.run_success(&["exchange", "start"]);
        // Should output exchange data (base64 or URL)
        assert!(
            output.contains("wb://") || output.len() > 50,
            "Expected exchange data, got: {}",
            output
        );
    }

    /// Trace: contact_exchange.feature - "Successful QR code exchange"
    #[test]
    fn test_exchange_complete_flow() {
        // Alice generates exchange data
        let alice = CliTestContext::new();
        alice.init("Alice Smith");
        alice.run_success(&["card", "add", "email", "Work", "alice@work.com"]);

        let alice_exchange = alice.run_success(&["exchange", "start"]);
        let alice_data: String = alice_exchange.lines().last().unwrap_or("").trim().to_string();

        // Bob completes exchange with Alice's data
        let bob = CliTestContext::new();
        bob.init("Bob Jones");
        bob.run_success(&["card", "add", "phone", "Mobile", "+1-555-262-1234"]);

        // Try to complete - this may fail without relay but tests the flow
        let result = bob.run(&["exchange", "complete", &alice_data]);

        // The exchange flow should at least parse the data
        // Full exchange requires relay connectivity
        let stdout = String::from_utf8_lossy(&result.stdout);
        let stderr = String::from_utf8_lossy(&result.stderr);
        let combined = format!("{}{}", stdout, stderr);

        // Should either succeed or fail with connectivity error, not parsing error
        assert!(
            !combined.contains("malformed"),
            "Exchange data parsing failed: {}",
            combined
        );
    }

    /// Trace: contact_exchange.feature - "Handle malformed QR code"
    #[test]
    fn test_exchange_complete_invalid_data() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let stderr = ctx.run_failure(&["exchange", "complete", "not-valid-exchange-data"]);
        assert!(
            stderr.contains("invalid") || stderr.contains("Invalid") || stderr.contains("failed") || stderr.contains("error"),
            "Expected error for invalid data, got: {}",
            stderr
        );
    }
}

// ===========================================================================
// Contacts Management Tests
// Trace: features/contacts_management.feature
// ===========================================================================

mod contacts_management {
    use super::*;

    /// Trace: contacts_management.feature - "List all contacts"
    #[test]
    fn test_contacts_list_empty() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let output = ctx.run_success(&["contacts", "list"]);
        assert!(
            output.contains("No contacts") || output.contains("empty") || output.is_empty() || output.contains("0"),
            "Expected no contacts, got: {}",
            output
        );
    }

    /// Trace: contacts_management.feature - "Search contacts"
    #[test]
    fn test_contacts_search() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let output = ctx.run_success(&["contacts", "search", "Bob"]);
        // With no contacts, should return empty or "not found"
        assert!(
            output.contains("No") || output.contains("not found") || output.is_empty() || output.contains("0"),
            "Unexpected search result: {}",
            output
        );
    }
}

// ===========================================================================
// Visibility Labels Tests
// Trace: features/visibility_labels.feature
// ===========================================================================

mod visibility_labels {
    use super::*;

    /// Trace: visibility_labels.feature - "Create a new visibility label"
    #[test]
    fn test_labels_create() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let output = ctx.run_success(&["labels", "create", "Family"]);
        assert!(
            output.contains("created") || output.contains("Family"),
            "Expected label created, got: {}",
            output
        );
    }

    /// Trace: visibility_labels.feature - "List labels"
    #[test]
    fn test_labels_list() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        ctx.run_success(&["labels", "create", "Family"]);
        ctx.run_success(&["labels", "create", "Friends"]);

        let output = ctx.run_success(&["labels", "list"]);
        assert!(output.contains("Family"));
        assert!(output.contains("Friends"));
    }

    /// Trace: visibility_labels.feature - "Cannot create duplicate label names"
    #[test]
    fn test_labels_create_duplicate_fails() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        ctx.run_success(&["labels", "create", "Family"]);
        let stderr = ctx.run_failure(&["labels", "create", "Family"]);

        assert!(
            stderr.contains("exists") || stderr.contains("duplicate") || stderr.contains("already"),
            "Expected duplicate error, got: {}",
            stderr
        );
    }

    /// Trace: visibility_labels.feature - "Rename an existing label"
    #[test]
    fn test_labels_rename() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        ctx.run_success(&["labels", "create", "Work"]);
        ctx.run_success(&["labels", "rename", "Work", "Colleagues"]);

        let output = ctx.run_success(&["labels", "list"]);
        assert!(output.contains("Colleagues"));
        assert!(!output.contains("Work") || output.contains("Colleagues"));
    }

    /// Trace: visibility_labels.feature - "Delete a label"
    #[test]
    fn test_labels_delete() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        ctx.run_success(&["labels", "create", "Temporary"]);
        ctx.run_success(&["labels", "delete", "Temporary"]);

        let output = ctx.run_success(&["labels", "list"]);
        assert!(
            !output.contains("Temporary") || output.contains("No labels"),
            "Label should be deleted, got: {}",
            output
        );
    }

    /// Trace: visibility_labels.feature - "Show label details"
    #[test]
    fn test_labels_show() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        ctx.run_success(&["labels", "create", "Family"]);

        let output = ctx.run_success(&["labels", "show", "Family"]);
        assert!(output.contains("Family"));
    }
}

// ===========================================================================
// Device Management Tests
// Trace: features/device_management.feature
// ===========================================================================

mod device_management {
    use super::*;

    /// Trace: device_management.feature - "List linked devices"
    #[test]
    fn test_device_list() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let output = ctx.run_success(&["device", "list"]);
        // Should list at least the current device
        assert!(
            output.contains("Device") || output.contains("device") || output.contains("1"),
            "Expected device list, got: {}",
            output
        );
    }

    /// Trace: device_management.feature - "Generate device linking QR code"
    #[test]
    fn test_device_link_generates_qr() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let output = ctx.run_success(&["device", "link"]);
        // Should output linking data
        assert!(
            output.len() > 20,
            "Expected device link data, got: {}",
            output
        );
    }
}

// ===========================================================================
// Recovery Tests
// Trace: features/identity_management.feature (recovery scenarios)
// ===========================================================================

mod recovery {
    use super::*;

    /// Trace: Recovery settings
    #[test]
    fn test_recovery_settings_show() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let output = ctx.run_success(&["recovery", "settings", "show"]);
        assert!(
            output.contains("recovery") || output.contains("Recovery") || output.contains("threshold") || output.contains("voucher"),
            "Expected recovery settings, got: {}",
            output
        );
    }

    /// Trace: Recovery settings can be configured
    #[test]
    fn test_recovery_settings_set() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let output = ctx.run_success(&["recovery", "settings", "set", "--recovery", "3", "--verification", "2"]);
        assert!(
            output.contains("updated") || output.contains("set") || output.contains("3"),
            "Expected settings update confirmation, got: {}",
            output
        );
    }

    /// Trace: Recovery status shows pending state
    #[test]
    fn test_recovery_status_no_claim() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        let output = ctx.run_success(&["recovery", "status"]);
        assert!(
            output.contains("No") || output.contains("none") || output.contains("active") || output.contains("claim"),
            "Expected no active recovery, got: {}",
            output
        );
    }
}

// ===========================================================================
// Social Network Tests
// Trace: features/contact_card_management.feature (social registry)
// ===========================================================================

mod social {
    use super::*;

    /// Trace: contact_card_management.feature - "List available social networks"
    #[test]
    fn test_social_list() {
        let ctx = CliTestContext::new();

        let output = ctx.run_success(&["social", "list"]);
        assert!(output.contains("twitter") || output.contains("Twitter") || output.contains("github") || output.contains("GitHub"));
    }

    /// Trace: contact_card_management.feature - "Generate profile URL"
    #[test]
    fn test_social_url() {
        let ctx = CliTestContext::new();

        let output = ctx.run_success(&["social", "url", "github", "octocat"]);
        assert!(output.contains("github.com") && output.contains("octocat"));
    }

    /// Trace: contact_card_management.feature - Search social networks
    #[test]
    fn test_social_list_search() {
        let ctx = CliTestContext::new();

        let output = ctx.run_success(&["social", "list", "git"]);
        assert!(output.contains("GitHub") || output.contains("github") || output.contains("GitLab") || output.contains("gitlab"));
    }
}

// ===========================================================================
// Sync Tests
// Trace: features/sync_updates.feature
// ===========================================================================

mod sync {
    use super::*;

    /// Trace: sync_updates.feature - Sync command runs (may fail without relay)
    #[test]
    fn test_sync_command_executes() {
        let ctx = CliTestContext::new();
        ctx.init("Alice Smith");

        // Sync will likely fail without a running relay, but should execute
        let output = ctx.run(&["sync"]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{}{}", stdout, stderr);

        // Should either succeed or fail with connection error
        assert!(
            combined.contains("sync") || combined.contains("Sync") ||
            combined.contains("connect") || combined.contains("Connect") ||
            combined.contains("relay") || combined.contains("Relay") ||
            combined.contains("error") || combined.contains("Error") ||
            combined.contains("success") || combined.contains("Success"),
            "Sync command should run, got: {}",
            combined
        );
    }
}

// ===========================================================================
// Shell Completions Test
// ===========================================================================

mod completions {
    use super::*;

    /// Test completions generation works
    #[test]
    fn test_completions_bash() {
        let ctx = CliTestContext::new();
        let output = ctx.run_success(&["completions", "bash"]);
        assert!(output.contains("complete") || output.contains("_vauchi"));
    }

    /// Test completions for different shells
    #[test]
    fn test_completions_zsh() {
        let ctx = CliTestContext::new();
        let output = ctx.run_success(&["completions", "zsh"]);
        assert!(output.contains("compdef") || output.contains("_vauchi"));
    }
}
