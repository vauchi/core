// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Content cache for storing downloaded content locally
//!
//! The cache stores content files and manifests in a local directory,
//! using atomic writes to prevent partial files on crash/interruption.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use thiserror::Error;

use super::integrity::{verify_checksum, IntegrityError};
use super::types::{ContentManifest, ContentType};

/// Local cache for remote content
pub struct ContentCache {
    cache_dir: PathBuf,
}

impl ContentCache {
    /// Create a new content cache at the given storage path
    ///
    /// Creates a `content/` subdirectory if it doesn't exist.
    pub fn new(storage_path: &Path) -> Result<Self, CacheError> {
        let cache_dir = storage_path.join("content");
        fs::create_dir_all(&cache_dir)?;
        Ok(Self { cache_dir })
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

    /// Get cached content file if it exists
    pub fn get_content(&self, content_type: ContentType, filename: &str) -> Option<Vec<u8>> {
        let path = self.content_path(content_type, filename);
        fs::read(&path).ok()
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

        let path = dir.join(filename);
        atomic_write(&path, data)
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

    fn content_path(&self, content_type: ContentType, filename: &str) -> PathBuf {
        self.cache_dir.join(content_type.dir_name()).join(filename)
    }
}

/// Atomic file write (write to temp, then rename)
///
/// This ensures that the file is never in a partial state - either the
/// old content remains or the new content is fully written.
fn atomic_write(path: &Path, data: &[u8]) -> Result<(), CacheError> {
    let temp_path = path.with_extension("tmp");

    // Write to temp file
    fs::write(&temp_path, data)?;

    // Atomic rename
    fs::rename(&temp_path, path)?;

    Ok(())
}

/// Errors that can occur with the content cache
#[derive(Debug, Error)]
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
