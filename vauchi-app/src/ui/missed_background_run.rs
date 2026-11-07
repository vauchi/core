// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A scheduler run that finds storage locked cannot write into the vault,
//! so it leaves one plain record beside the database: the time, and
//! nothing else (vauchi/private#1093 slice 3). The next open takes it into
//! memory and deletes it, so it stays on disk only while the phone is
//! locked.

use std::io::Read;
use std::path::Path;

const FILE_NAME: &str = "missed_background_run";
/// `u64::MAX` has 20 digits; one more byte tells an oversized file apart.
const MAX_FILE_BYTES: u64 = 21;

/// Records that a scheduler run at `at_unix_seconds` could not open
/// storage, replacing any earlier record.
pub fn record_missed_background_run(data_dir: &Path, at_unix_seconds: u64) -> std::io::Result<()> {
    let path = data_dir.join(FILE_NAME);
    let partial = path.with_extension("partial");
    std::fs::write(&partial, at_unix_seconds.to_string())?;
    // A rename replaces the record whole, so a run killed mid-write leaves
    // the previous one.
    std::fs::rename(partial, path)
}

/// The recorded time, if a whole one is there. Whatever is on disk under
/// the record's name is removed; a record that cannot be removed is not
/// shown, so it is never shown twice.
// Its caller in `ui::app_engine` is compiled out of the no-default-features
// build.
#[cfg_attr(not(feature = "network-rustls"), allow(dead_code))]
pub(crate) fn take_missed_background_run(data_dir: &Path) -> Option<u64> {
    let path = data_dir.join(FILE_NAME);
    let at = read_record(&path);
    if path.is_file() {
        std::fs::remove_file(&path).ok()?;
    }
    at
}

fn read_record(path: &Path) -> Option<u64> {
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(MAX_FILE_BYTES).read_to_string(&mut text).ok()?;
    let at = text.parse::<u64>().ok()?;
    (at.to_string() == text).then_some(at)
}

// INLINE_TEST_REQUIRED: the adversarial tests write raw bytes under the
// crate-private FILE_NAME and call the crate-private take, neither reachable
// from external tests/.
#[cfg(test)]
#[path = "missed_background_run_tests.rs"]
mod tests;
