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

pub(crate) fn ensure_no_ambiguous_backups(database: &Path) -> Result<(), StorageError> {
    let invalid_name = || StorageError::InvalidData("invalid database filename".into());
    let name = database.file_name().ok_or_else(invalid_name)?;
    let stem = database.file_stem().ok_or_else(invalid_name)?;
    let mut stem_prefix = stem.to_os_string();
    stem_prefix.push(".pre-migration-v");
    let mut name_prefix = name.to_os_string();
    name_prefix.push(".pre-migration-v");
    let directory = database
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    for entry in std::fs::read_dir(directory).map_err(inspection_error)? {
        let name = entry.map_err(inspection_error)?.file_name();
        if matches_backup(&name, &stem_prefix) || matches_backup(&name, &name_prefix) {
            // #1178: both legacy spellings can refer to another database's recovery copy.
            return Err(StorageError::InvalidData(
                "migration backup ownership is ambiguous; recovery data was preserved".into(),
            ));
        }
    }
    Ok(())
}
