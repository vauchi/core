// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fs::{DirBuilder, File};
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

#[derive(Debug)]
pub(crate) enum SnapshotFailure {
    Io(io::ErrorKind),
    Sqlite(Option<i32>),
    InvalidPath,
}

impl From<io::Error> for SnapshotFailure {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

impl SnapshotFailure {
    pub(crate) fn sqlite_code(&self) -> Option<i32> {
        match self {
            Self::Sqlite(code) => *code,
            _ => None,
        }
    }

    pub(crate) fn io_kind(&self) -> Option<io::ErrorKind> {
        match self {
            Self::Io(kind) => Some(*kind),
            _ => None,
        }
    }
}

struct StagingDirectory(PathBuf);

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        for result in [
            std::fs::remove_file(self.0.join("snapshot.db")),
            std::fs::remove_dir(&self.0),
        ] {
            if let Err(error) = result
                && error.kind() != io::ErrorKind::NotFound
            {
                tracing::warn!(target: "vauchi.storage.migration", io_error_kind = ?error.kind(), "could not remove snapshot staging data");
            }
        }
    }
}

pub(crate) fn create(
    conn: &Connection,
    database: &Path,
    version: u32,
) -> Result<PathBuf, SnapshotFailure> {
    let parent = database
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = database.file_name().ok_or(SnapshotFailure::InvalidPath)?;
    let mut destination = name.to_os_string();
    destination.push(format!(".pre-migration-v{version}.bak"));
    let destination = parent.join(destination);

    let directory = parent.join(format!(".vauchi-migration-{}", uuid::Uuid::new_v4()));
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&directory)?;
    let staging = StagingDirectory(directory);
    let snapshot = staging.0.join("snapshot.db");
    let sql_path = snapshot.to_str().ok_or(SnapshotFailure::InvalidPath)?;
    conn.execute("VACUUM INTO ?1", [sql_path])
        .map_err(|error| SnapshotFailure::Sqlite(error.sqlite_error().map(|e| e.extended_code)))?;
    // #1178: Windows FlushFileBuffers requires a handle opened for writing.
    File::options().write(true).open(&snapshot)?.sync_all()?;
    // #1178: publication must fail if another file or alias already owns the name.
    std::fs::hard_link(&snapshot, &destination)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(destination)
}
