// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Self-healing anti-pattern tests (Phase 8 audit hardening).
//!
//! These tests scan source code for patterns that caused audit findings.
//! They run as part of `just check core` and fail when someone reintroduces
//! a previously-fixed anti-pattern. Each test documents which finding it guards.

use std::path::Path;

/// Recursively collect all `.rs` files under a directory, excluding test files.
fn collect_source_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip target/ and tests/
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name != "target" && name != "tests" {
                    collect_source_files(&path, files);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
}

fn source_files() -> Vec<std::path::PathBuf> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_source_files(&src, &mut files);
    files
}

/// F3 guard: no `unwrap_or(encrypted)` or `unwrap_or(plaintext)` in storage decrypt paths.
///
/// Silent plaintext fallback on decryption failure masks real errors. Post-migration,
/// all _encrypted columns contain valid ciphertext. Decrypt failures must propagate.
// @scenario: security :: No silent decrypt fallback in storage
#[test]
fn test_no_silent_decrypt_fallback_in_storage() {
    let mut violations = Vec::new();

    for path in source_files() {
        let path_str = path.to_string_lossy();
        if !path_str.contains("storage") {
            continue;
        }

        let content = std::fs::read_to_string(&path).unwrap();
        for (line_num, line) in content.lines().enumerate() {
            if line.contains("decrypt") && line.contains("unwrap_or") {
                violations.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    line_num + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "F3 recurrence: decrypt().unwrap_or() silently swallows errors.\n\
         Use .map_err()? instead. Violations:\n  {}",
        violations.join("\n  ")
    );
}

/// F7 guard: no `from_bytes_unchecked` outside crypto/ module.
///
/// `from_bytes_unchecked` accepts all-zeros keys. At trust boundaries (FFI, file I/O),
/// use `try_from_bytes()` to reject degenerate keys. Only crypto internals (HKDF output,
/// post-decryption) may use unchecked because the input is mathematically non-degenerate.
// @scenario: security :: No unchecked key construction at trust boundaries
#[test]
fn test_no_unchecked_key_construction_outside_crypto() {
    let mut violations = Vec::new();

    for path in source_files() {
        let path_str = path.to_string_lossy();
        // Allow in crypto/ module (HKDF output, chain key derivation)
        if path_str.contains("/crypto/") {
            continue;
        }
        // Allow in storage/rekey.rs (per-contact derived keys from HKDF)
        if path_str.ends_with("rekey.rs") {
            continue;
        }
        // Allow in contact_row.rs and device_sync.rs (post-decryption — AEAD-verified bytes)
        if path_str.ends_with("contact_row.rs") || path_str.ends_with("device_sync.rs") {
            continue;
        }

        let content = std::fs::read_to_string(&path).unwrap();
        for (line_num, line) in content.lines().enumerate() {
            if line.contains("from_bytes_unchecked") && !line.trim_start().starts_with("//") {
                violations.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    line_num + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "F7 recurrence: from_bytes_unchecked() at trust boundary.\n\
         Use try_from_bytes() to reject degenerate keys. Violations:\n  {}",
        violations.join("\n  ")
    );
}
