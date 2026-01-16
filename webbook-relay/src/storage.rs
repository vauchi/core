//! Blob Storage
//!
//! In-memory storage for encrypted blobs awaiting delivery.
//!
//! Note: Current implementation is in-memory and does not survive server restarts.
//! For production deployments with long TTLs (90 days), consider adding persistent
//! storage (SQLite or RocksDB) to preserve messages across restarts.

use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A stored encrypted blob.
#[derive(Debug, Clone)]
pub struct StoredBlob {
    /// Unique blob ID.
    pub id: String,
    /// Sender's identity (for tracking, not revealed to recipient).
    pub sender_id: String,
    /// The encrypted data (opaque to the relay).
    pub data: Vec<u8>,
    /// When the blob was stored (Unix timestamp in seconds).
    pub created_at_secs: u64,
}

impl StoredBlob {
    /// Creates a new stored blob.
    pub fn new(sender_id: String, data: Vec<u8>) -> Self {
        let created_at_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        StoredBlob {
            id: uuid::Uuid::new_v4().to_string(),
            sender_id,
            data,
            created_at_secs,
        }
    }

    /// Returns the age of this blob.
    fn age_secs(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.created_at_secs)
    }

    /// Checks if the blob has expired.
    pub fn is_expired(&self, ttl: Duration) -> bool {
        // Use >= so that TTL of 0 means immediately expired
        self.age_secs() >= ttl.as_secs()
    }
}

/// In-memory storage for blobs indexed by recipient ID.
pub struct BlobStorage {
    /// Blobs waiting for each recipient.
    blobs: RwLock<HashMap<String, VecDeque<StoredBlob>>>,
}

impl BlobStorage {
    /// Creates a new empty storage.
    pub fn new() -> Self {
        BlobStorage {
            blobs: RwLock::new(HashMap::new()),
        }
    }

    /// Stores a blob for a recipient.
    pub fn store(&self, recipient_id: &str, blob: StoredBlob) {
        let mut blobs = self.blobs.write().unwrap();
        blobs
            .entry(recipient_id.to_string())
            .or_default()
            .push_back(blob);
    }

    /// Retrieves all pending blobs for a recipient (without removing them).
    pub fn peek(&self, recipient_id: &str) -> Vec<StoredBlob> {
        let blobs = self.blobs.read().unwrap();
        blobs
            .get(recipient_id)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Retrieves and removes all pending blobs for a recipient.
    #[allow(dead_code)]
    pub fn take(&self, recipient_id: &str) -> Vec<StoredBlob> {
        let mut blobs = self.blobs.write().unwrap();
        blobs
            .remove(recipient_id)
            .map(|q| q.into_iter().collect())
            .unwrap_or_default()
    }

    /// Acknowledges receipt of a specific blob (removes it).
    ///
    /// Returns true if the blob was found and removed.
    pub fn acknowledge(&self, recipient_id: &str, blob_id: &str) -> bool {
        let mut blobs = self.blobs.write().unwrap();
        if let Some(queue) = blobs.get_mut(recipient_id) {
            let initial_len = queue.len();
            queue.retain(|b| b.id != blob_id);
            let removed = queue.len() < initial_len;

            // Clean up empty queues
            if queue.is_empty() {
                blobs.remove(recipient_id);
            }

            removed
        } else {
            false
        }
    }

    /// Removes all expired blobs.
    ///
    /// Returns the number of blobs removed.
    pub fn cleanup_expired(&self, ttl: Duration) -> usize {
        let mut blobs = self.blobs.write().unwrap();
        let mut removed = 0;

        // Collect keys to avoid borrowing issues
        let keys: Vec<String> = blobs.keys().cloned().collect();

        for key in keys {
            if let Some(queue) = blobs.get_mut(&key) {
                let initial_len = queue.len();
                queue.retain(|b| !b.is_expired(ttl));
                removed += initial_len - queue.len();

                // Clean up empty queues
                if queue.is_empty() {
                    blobs.remove(&key);
                }
            }
        }

        removed
    }

    /// Returns the total number of stored blobs.
    #[allow(dead_code)]
    pub fn blob_count(&self) -> usize {
        let blobs = self.blobs.read().unwrap();
        blobs.values().map(|q| q.len()).sum()
    }

    /// Returns the number of recipients with pending blobs.
    #[allow(dead_code)]
    pub fn recipient_count(&self) -> usize {
        let blobs = self.blobs.read().unwrap();
        blobs.len()
    }
}

impl Default for BlobStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_peek() {
        let storage = BlobStorage::new();

        let blob = StoredBlob::new("sender-1".to_string(), vec![1, 2, 3]);
        let blob_id = blob.id.clone();

        storage.store("recipient-1", blob);

        let peeked = storage.peek("recipient-1");
        assert_eq!(peeked.len(), 1);
        assert_eq!(peeked[0].id, blob_id);
        assert_eq!(peeked[0].data, vec![1, 2, 3]);

        // Peek doesn't remove
        let peeked_again = storage.peek("recipient-1");
        assert_eq!(peeked_again.len(), 1);
    }

    #[test]
    fn test_take() {
        let storage = BlobStorage::new();

        storage.store("recipient-1", StoredBlob::new("sender-1".to_string(), vec![1]));
        storage.store("recipient-1", StoredBlob::new("sender-2".to_string(), vec![2]));

        let taken = storage.take("recipient-1");
        assert_eq!(taken.len(), 2);

        // Take removes all
        let taken_again = storage.take("recipient-1");
        assert!(taken_again.is_empty());
    }

    #[test]
    fn test_acknowledge() {
        let storage = BlobStorage::new();

        let blob1 = StoredBlob::new("sender-1".to_string(), vec![1]);
        let blob2 = StoredBlob::new("sender-2".to_string(), vec![2]);
        let blob1_id = blob1.id.clone();

        storage.store("recipient-1", blob1);
        storage.store("recipient-1", blob2);

        // Acknowledge first blob
        let removed = storage.acknowledge("recipient-1", &blob1_id);
        assert!(removed);

        // Only one blob remains
        let remaining = storage.peek("recipient-1");
        assert_eq!(remaining.len(), 1);
        assert_ne!(remaining[0].id, blob1_id);
    }

    #[test]
    fn test_acknowledge_nonexistent() {
        let storage = BlobStorage::new();

        storage.store("recipient-1", StoredBlob::new("sender-1".to_string(), vec![1]));

        let removed = storage.acknowledge("recipient-1", "nonexistent-id");
        assert!(!removed);

        let removed = storage.acknowledge("nonexistent-recipient", "any-id");
        assert!(!removed);
    }

    #[test]
    fn test_cleanup_expired() {
        let storage = BlobStorage::new();

        // Store a blob
        storage.store("recipient-1", StoredBlob::new("sender-1".to_string(), vec![1]));

        // With a long TTL, nothing should be removed
        let removed = storage.cleanup_expired(Duration::from_secs(3600));
        assert_eq!(removed, 0);
        assert_eq!(storage.blob_count(), 1);

        // With zero TTL, everything should be removed
        let removed = storage.cleanup_expired(Duration::ZERO);
        assert_eq!(removed, 1);
        assert_eq!(storage.blob_count(), 0);
    }

    #[test]
    fn test_blob_count() {
        let storage = BlobStorage::new();

        assert_eq!(storage.blob_count(), 0);

        storage.store("recipient-1", StoredBlob::new("sender-1".to_string(), vec![1]));
        storage.store("recipient-1", StoredBlob::new("sender-2".to_string(), vec![2]));
        storage.store("recipient-2", StoredBlob::new("sender-3".to_string(), vec![3]));

        assert_eq!(storage.blob_count(), 3);
        assert_eq!(storage.recipient_count(), 2);
    }

    #[test]
    fn test_peek_nonexistent_recipient() {
        let storage = BlobStorage::new();
        let peeked = storage.peek("nonexistent");
        assert!(peeked.is_empty());
    }
}
