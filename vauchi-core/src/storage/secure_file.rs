// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, Seek, Write};
use std::path::Path;

fn unsafe_entry() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "refusing an unsafe overwrite target",
    )
}

#[cfg(unix)]
fn check_handle(before: &Metadata, file: &File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let opened = file.metadata()?;
    if !opened.is_file()
        || opened.nlink() != 1
        || (before.dev(), before.ino()) != (opened.dev(), opened.ino())
    {
        return Err(unsafe_entry());
    }
    Ok(())
}

#[cfg(windows)]
fn check_handle(_: &Metadata, file: &File) -> io::Result<()> {
    const REPARSE_POINT: u64 = 0x400;
    let info = winapi_util::file::information(file)?;
    if !file.metadata()?.is_file()
        || info.number_of_links() != 1
        || info.file_attributes() & REPARSE_POINT != 0
    {
        return Err(unsafe_entry());
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn check_handle(_: &Metadata, _: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "overwrite target verification unavailable",
    ))
}

/// Overwrites a single-link regular file through a verified handle.
///
/// Alias entries are preserved for the caller's namespace cleanup, never
/// followed for overwrite. The parent directory must not have hostile
/// concurrent writers; handle verification is not a directory capability.
pub(crate) fn secure_overwrite_file(path: &Path) -> io::Result<()> {
    let before = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !before.is_file() || before.file_type().is_symlink() {
        return Err(unsafe_entry());
    }
    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // #1178: inspect reparse points themselves and deny concurrent handle opens.
        const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(OPEN_REPARSE_POINT).share_mode(0);
    }
    let mut file = options.open(path)?;
    check_handle(&before, &file)?;
    let size = file.metadata()?.len();
    let mut buffer = [0u8; 8192];
    for random in [true, false] {
        file.rewind()?;
        let mut remaining = size;
        while remaining != 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            if random {
                crate::crypto::random_fill(&mut buffer[..count]);
            } else {
                buffer[..count].fill(0);
            }
            file.write_all(&buffer[..count])?;
            remaining -= count as u64;
        }
        file.sync_all()?;
    }
    let current = std::fs::symlink_metadata(path)?;
    if !current.is_file() || current.file_type().is_symlink() {
        return Err(unsafe_entry());
    }
    check_handle(&current, &file)?;
    drop(file);
    std::fs::remove_file(path)
}
