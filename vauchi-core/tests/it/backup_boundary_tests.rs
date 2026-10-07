// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Backup import size limits sit exactly where documented, and backup
//! debug output never carries identity secrets or contact data
//! (vauchi/private#522).

use vauchi_core::backup::{BackupError, import_contact_backup, import_full_backup};

const THIRTY_TWO_MIB: usize = 32 * 1024 * 1024;
const CONTACT_BACKUP_VERSION: u8 = 0x01;
const FULL_BACKUP_VERSION: u8 = 0x03;

fn blob(version: u8, len: usize) -> Vec<u8> {
    let mut data = vec![0u8; len];
    data[0] = version;
    data
}

// Inputs at the documented edges get past the size checks and fail only on
// authentication; one byte beyond either edge is refused unopened.
// @internal
#[test]
fn a_contact_backup_import_accepts_sizes_from_17_bytes_to_32_mib() {
    let import = |len| import_contact_backup(&blob(CONTACT_BACKUP_VERSION, len), "pw");

    assert!(matches!(import(16), Err(BackupError::TooShort)));
    assert!(matches!(import(17), Err(BackupError::DecryptionFailed)));
    assert!(matches!(
        import(THIRTY_TWO_MIB),
        Err(BackupError::DecryptionFailed)
    ));
    assert!(matches!(
        import(THIRTY_TWO_MIB + 1),
        Err(BackupError::TooLarge)
    ));
}

// @internal
#[test]
fn a_full_backup_import_accepts_sizes_from_17_bytes_to_32_mib() {
    let import = |len| import_full_backup(&blob(FULL_BACKUP_VERSION, len), "pw");

    assert!(matches!(import(16), Err(BackupError::TooShort)));
    assert!(matches!(import(17), Err(BackupError::DecryptionFailed)));
    assert!(matches!(
        import(THIRTY_TWO_MIB),
        Err(BackupError::DecryptionFailed)
    ));
    assert!(matches!(
        import(THIRTY_TWO_MIB + 1),
        Err(BackupError::TooLarge)
    ));
}

// Debug output can reach logs and crash reports.
// @internal
#[test]
fn backup_debug_output_carries_no_secrets_or_contact_data() {
    use vauchi_core::backup::full_backup::{
        BackupSections, FullBackupEnvelope, IdentitySection, LabelSection,
    };

    let label = LabelSection {
        label_id: "label-id-secret".into(),
        name: "label-name-secret".into(),
        contacts: vec!["label-member-secret".into()],
    };
    let envelope = FullBackupEnvelope {
        version: 3,
        created_at: 1_700_000_000,
        sections: BackupSections {
            identity: IdentitySection {
                display_name: "Alice".into(),
                master_seed_b64: "seed-secret".into(),
                device_index: 4,
                device_name: "Phone".into(),
            },
            contacts: vec![serde_json::json!({"name": "contact-secret"})],
            own_card: None,
            labels: vec![],
        },
    };

    let envelope_text = format!("{envelope:?}");
    let label_text = format!("{label:?}");

    for expected in [
        "version: 3",
        "created_at: 1700000000",
        "Alice",
        "device_index: 4",
    ] {
        assert!(
            envelope_text.contains(expected),
            "{expected} in {envelope_text}"
        );
    }
    assert!(label_text.contains("[REDACTED]"), "{label_text}");
    for text in [&envelope_text, &label_text] {
        assert!(!text.contains("secret"), "{text}");
    }
}
