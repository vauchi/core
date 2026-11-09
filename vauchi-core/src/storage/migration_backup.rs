// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::ffi::OsStr;
use std::path::Path;

use super::StorageError;

fn inspection_error(error: std::io::Error) -> StorageError {
    StorageError::InvalidData(format!(
        "could not inspect recovery files: {}",
        error.kind()
    ))
}

fn matches_backup(name: &OsStr, prefix: &OsStr) -> bool {
    name.as_encoded_bytes()
        .strip_prefix(prefix.as_encoded_bytes())
        .and_then(|suffix| suffix.strip_suffix(b".bak"))
        .and_then(|version| std::str::from_utf8(version).ok())
        .is_some_and(|version| {
            version
                .parse::<u32>()
                .is_ok_and(|parsed| parsed.to_string() == version)
        })
}

pub(crate) fn snapshot_paths(database: &Path) -> Result<Vec<std::path::PathBuf>, StorageError> {
    let invalid_name = || StorageError::InvalidData("invalid database filename".into());
    let name = database.file_name().ok_or_else(invalid_name)?;
    let mut name_prefix = name.to_os_string();
    name_prefix.push(".pre-migration-v");
    let directory = database
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && super::secure_file::confirmed_absent(directory) =>
        {
            return Ok(Vec::new());
        }
        Err(error) => return Err(inspection_error(error)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(inspection_error)?;
        if matches_backup(&entry.file_name(), &name_prefix) {
            paths.push(entry.path());
        }
    }
    Ok(paths)
}

pub(crate) fn ensure_no_ambiguous_backups(database: &Path) -> Result<(), StorageError> {
    if !snapshot_paths(database)?.is_empty() {
        return Err(StorageError::InvalidData(
            "migration backup ownership is ambiguous; recovery data was preserved".into(),
        ));
    }
    Ok(())
}
