// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Content cache for storing downloaded content locally
//!
//! The cache stores content files and manifests in a local directory,
//! using atomic writes to prevent partial files on crash/interruption.
//! Content files are stored gzipped (vauchi/private#733 D9).

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use thiserror::Error;

use super::integrity::{IntegrityError, verify_checksum};
use super::types::{ContentManifest, ContentType};

/// Local cache for remote content
pub struct ContentCache {
    cache_dir: PathBuf,
    max_content_size: u64,
}

const STORED_SUFFIX: &str = ".gz";

impl ContentCache {
    /// Create a new content cache at the given storage path
    ///
    /// Creates a `content/` subdirectory if it doesn't exist.
    pub fn new(storage_path: &Path) -> Result<Self, CacheError> {
        let cache_dir = storage_path.join("content");
        fs::create_dir_all(&cache_dir)?;
        Ok(Self {
            cache_dir,
            max_content_size: super::ContentConfig::default().max_content_size,
        })
    }

    /// Cap on the inflated size of a stored file, so a stored file cannot
    /// expand into more memory than a download may take (DC-03).
    pub fn with_max_content_size(mut self, max_content_size: u64) -> Self {
        self.max_content_size = max_content_size;
        self
    }

    /// Get cached manifest if it exists
    pub fn get_manifest(&self) -> Option<ContentManifest> {
        let path = self.cache_dir.join("manifest.json");
        let data = fs::read_to_string(&path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Save manifest to cache
    pub fn save_manifest(&self, manifest: &ContentManifest) -> Result<(), CacheError> {
        let path = self.cache_dir.join("manifest.json");
        let data = serde_json::to_string_pretty(manifest)?;
        atomic_write(&path, data.as_bytes())
    }

    /// Get cached content file if it exists. A stored file that does not
    /// inflate, or inflates past the size cap, is dropped with the manifest
    /// so the next update cycle downloads it again.
    pub fn get_content(&self, content_type: ContentType, filename: &str) -> Option<Vec<u8>> {
        let path = self.content_path(content_type, filename);
        let stored = fs::File::open(&path).ok()?;
        let mut data = Vec::new();
        let inflated = flate2::read::GzDecoder::new(stored)
            .take(self.max_content_size.saturating_add(1))
            .read_to_end(&mut data);
        if inflated.is_err() || data.len() as u64 > self.max_content_size || data.is_empty() {
            self.drop_stored(&path);
            return None;
        }
        Some(data)
    }

    /// The names of the cached files of one content type.
    pub fn content_files(&self, content_type: ContentType) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.content_dir(content_type)) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()?
                    .strip_suffix(STORED_SUFFIX)
                    .map(str::to_string)
            })
            .collect()
    }

    fn drop_stored(&self, path: &Path) {
        let dropped = remove_if_present(path).and_then(|()| self.clear_manifest());
        if let Err(e) = dropped {
            tracing::warn!("[Content] Failed to drop an unreadable cache entry: {e}");
        }
    }

    /// Save content file to cache with checksum verification
    ///
    /// The file is written atomically (to a temp file, then renamed) to prevent
    /// partial files on crash/interruption. Checksum is verified before saving.
    pub fn save_content(
        &self,
        content_type: ContentType,
        filename: &str,
        data: &[u8],
        checksum: &str,
    ) -> Result<(), CacheError> {
        // Verify checksum before saving
        verify_checksum(data, checksum)?;

        let dir = self.cache_dir.join(content_type.dir_name());
        fs::create_dir_all(&dir)?;

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(data)?;
        atomic_write(
            &self.content_path(content_type, filename),
            &encoder.finish()?,
        )
    }

    /// Remove one cached content file. A file that is already gone is not
    /// an error.
    pub fn remove_content(
        &self,
        content_type: ContentType,
        filename: &str,
    ) -> Result<(), CacheError> {
        remove_if_present(&self.content_path(content_type, filename))
    }

    /// Forget the cached manifest, so the next update cycle treats every
    /// content type as not downloaded.
    pub fn clear_manifest(&self) -> Result<(), CacheError> {
        remove_if_present(&self.cache_dir.join("manifest.json"))
    }

    /// Clear all content of a specific type
    pub fn clear_content_type(&self, content_type: ContentType) -> Result<(), CacheError> {
        let dir = self.cache_dir.join(content_type.dir_name());
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        Ok(())
    }

    /// Get the last time updates were checked
    pub fn get_last_check_time(&self) -> Option<SystemTime> {
        let path = self.cache_dir.join("last_check");
        let data = fs::read_to_string(&path).ok()?;
        let secs: u64 = data.trim().parse().ok()?;
        Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs))
    }

    /// Set the last time updates were checked
    pub fn set_last_check_time(&self, time: SystemTime) -> Result<(), CacheError> {
        let path = self.cache_dir.join("last_check");
        let secs = time
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| CacheError::InvalidTime)?
            .as_secs();
        atomic_write(&path, secs.to_string().as_bytes())
    }

    /// Get the directory path for a content type
    pub fn content_dir(&self, content_type: ContentType) -> PathBuf {
        self.cache_dir.join(content_type.dir_name())
    }

    fn content_path(&self, content_type: ContentType, filename: &str) -> PathBuf {
        self.content_dir(content_type)
            .join(format!("{filename}{STORED_SUFFIX}"))
    }
}

fn remove_if_present(path: &Path) -> Result<(), CacheError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Atomic file write (write to temp, then rename)
///
/// This ensures that the file is never in a partial state - either the
/// old content remains or the new content is fully written.
fn atomic_write(path: &Path, data: &[u8]) -> Result<(), CacheError> {
    let temp_path = path.with_extension("tmp");

    fs::write(&temp_path, data)?;

    // Atomic rename
    fs::rename(&temp_path, path)?;

    Ok(())
}

/// Errors that can occur with the content cache
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CacheError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// JSON serialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Integrity verification failed
    #[error("Integrity error: {0}")]
    Integrity(#[from] IntegrityError),

    /// Invalid time value
    #[error("Invalid time value")]
    InvalidTime,
}

// INLINE_TEST_REQUIRED: tests use private cache internals (atomic_write, CacheEntry)
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_atomic_write() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");

        atomic_write(&path, b"hello").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");

        // No temp file should remain
        assert!(!path.with_extension("tmp").exists());
    }
}
