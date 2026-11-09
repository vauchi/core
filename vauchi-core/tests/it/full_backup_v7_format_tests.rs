// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The v7 full backup is two payloads of one size (ADR-080 point 7,
//! vauchi/private#1117): a real one under the backup password and a decoy
//! one, both bound to the clear header, which holds a random file id and
//! one salt per payload. Its layout is published in the spec (D4), so
//! these tests read the bytes as an independent reader would.

use sha2::{Digest, Sha256};
use vauchi_core::{ContactField, FieldType, Vauchi};

const PASSWORD: &str = "correct-horse-battery-staple-v7";
const OTHER_PASSWORD: &str = "a-different-strong-password-v7";
const VERSION: u8 = 7;
const HEADER_LENGTH: usize = 1 + 16 + 16 + 16;
const AEAD_OVERHEAD: usize = 1 + 24 + 16;
const SMALLEST_BUCKET: usize = 4096;

struct File {
    bytes: Vec<u8>,
}

impl File {
    fn header(&self) -> &[u8] {
        &self.bytes[..HEADER_LENGTH]
    }
    fn file_id(&self) -> &[u8] {
        &self.bytes[1..17]
    }
    fn payloads(&self) -> (&[u8], &[u8]) {
        self.bytes[HEADER_LENGTH..].split_at((self.bytes.len() - HEADER_LENGTH) / 2)
    }
}

fn alice() -> Vauchi {
    let mut alice = Vauchi::in_memory().expect("install");
    alice.create_identity("Alice").expect("identity");
    alice
}

fn export(vauchi: &Vauchi, password: &str) -> File {
    let hex_file = vauchi.export_full_backup(password).expect("export");
    File {
        bytes: hex::decode(hex_file).expect("hex"),
    }
}

fn restores(bytes: &[u8], password: &str) -> Result<(), String> {
    let mut fresh = Vauchi::in_memory().expect("fresh install");
    fresh
        .import_full_backup(&hex::encode(bytes), password)
        .map_err(|e| e.to_string())
}

fn wrong_password_error(file: &File) -> String {
    restores(&file.bytes, "not-the-password-of-this-file").expect_err("a wrong password is refused")
}

// @scenario: backup_format_versioning :: Version byte identifies backup format
#[test]
fn a_full_backup_is_two_payloads_of_one_bucket_size() {
    let file = export(&alice(), PASSWORD);

    assert_eq!(file.bytes[0], VERSION);
    let (first, second) = file.payloads();
    assert_eq!(first.len(), second.len());
    let bucket = first.len() - AEAD_OVERHEAD;
    assert!(
        bucket.is_power_of_two(),
        "bucket {bucket} is a power of two"
    );
    assert!(
        bucket >= SMALLEST_BUCKET,
        "bucket {bucket} is at least the smallest"
    );
    assert_eq!(restores(&file.bytes, PASSWORD), Ok(()));
}

/// Forty own-card fields of hex digests: incompressible enough that the
/// real profile no longer fits the smallest bucket.
fn alice_with_a_large_card() -> Vauchi {
    let alice = alice();
    for i in 0..40u32 {
        let value: String = (0..28u32)
            .map(|j| hex::encode(Sha256::digest((i * 1000 + j).to_be_bytes())))
            .collect::<String>()[..900]
            .to_string();
        alice
            .add_own_field(ContactField::new(
                FieldType::Custom,
                &format!("note {i}"),
                &value,
                u64::from(i),
            ))
            .expect("field");
    }
    alice
}

fn bucket_of(file: &File) -> usize {
    file.payloads().0.len() - AEAD_OVERHEAD
}

// @scenario: backup_format_versioning :: Version byte identifies backup format
#[test]
fn a_larger_real_profile_moves_both_payloads_to_a_larger_bucket() {
    let small = export(&alice(), PASSWORD);

    let large = export(&alice_with_a_large_card(), PASSWORD);

    assert_eq!(bucket_of(&small), SMALLEST_BUCKET);
    let (first, second) = large.payloads();
    assert_eq!(first.len(), second.len());
    assert!(bucket_of(&large).is_power_of_two());
    assert!(
        bucket_of(&large) > SMALLEST_BUCKET,
        "a large card leaves the smallest bucket"
    );
    assert_eq!(restores(&large.bytes, PASSWORD), Ok(()));
}

// @scenario: backup_format_versioning :: Version byte identifies backup format
#[test]
fn every_export_draws_a_fresh_file_id_salts_and_payloads() {
    let install = alice();

    let first = export(&install, PASSWORD);
    let second = export(&install, PASSWORD);

    assert_ne!(first.file_id(), second.file_id());
    assert_ne!(first.header()[17..], second.header()[17..]);
    let (a1, b1) = first.payloads();
    let (a2, b2) = second.payloads();
    for (old, new) in [(a1, a2), (a1, b2), (b1, a2), (b1, b2)] {
        assert_ne!(old, new);
    }
}

// @scenario: backup_format_versioning :: Corrupted backup is detected
#[test]
fn payloads_from_another_file_do_not_open_under_this_header() {
    let install = alice();
    let first = export(&install, PASSWORD);
    let second = export(&install, PASSWORD);
    assert_eq!(
        first.bytes.len(),
        second.bytes.len(),
        "precondition: one bucket"
    );

    let mut spliced = first.header().to_vec();
    spliced.extend_from_slice(&second.bytes[HEADER_LENGTH..]);

    assert_eq!(
        restores(&spliced, PASSWORD),
        Err(wrong_password_error(&first))
    );
}

// @scenario: backup_format_versioning :: Corrupted backup is detected
#[test]
fn a_changed_file_id_fails_as_a_wrong_password_does() {
    let file = export(&alice(), PASSWORD);
    let mut tampered = file.bytes.clone();
    tampered[1] ^= 0x01;

    assert_eq!(
        restores(&tampered, PASSWORD),
        Err(wrong_password_error(&file))
    );
}

// @scenario: backup_format_versioning :: Restore v2 backup with wrong password
#[test]
fn another_password_opens_neither_payload() {
    let file = export(&alice(), PASSWORD);

    assert_eq!(
        restores(&file.bytes, OTHER_PASSWORD),
        Err(wrong_password_error(&file))
    );
}
