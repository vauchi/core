// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stable per-installation identifier.
//!
//! Each install of a vauchi frontend reads or creates a UUID at
//! `<data_dir>/install_id` on first launch. This identifier is used to derive
//! stable OS-keychain entry names that survive `data_dir` rename or relocation.
//!
//! Without this stability, frontends that hash the data_dir path into the
//! keychain entry name (e.g. CLI/TUI's earlier FNV-1a scheme) lose access to
//! their storage key the moment the user moves the data directory.

use std::fs;
use std::io;
use std::path::Path;

use uuid::Uuid;

/// Filename of the install-id marker inside the data directory.
const INSTALL_ID_FILE: &str = "install_id";

/// Reads the existing install-id from `<data_dir>/install_id`, or creates a
/// new UUIDv4 if the file does not exist.
///
/// The file is created with mode `0o600` on Unix. If the file exists but does
/// not contain a parseable UUID this function returns an error rather than
/// regenerating — silent regeneration would orphan the existing OS-keychain
/// entry derived from the prior id.
pub fn read_or_create_install_id(data_dir: &Path) -> io::Result<String> {
    let path = data_dir.join(INSTALL_ID_FILE);

    if path.exists() {
        let raw = fs::read_to_string(&path)?;
        let trimmed = raw.trim();
        Uuid::parse_str(trimmed).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid install_id at {}: {e}", path.display()),
            )
        })?;
        return Ok(trimmed.to_string());
    }

    fs::create_dir_all(data_dir)?;

    let id = Uuid::new_v4().to_string();
    fs::write(&path, id.as_bytes())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(id)
}
