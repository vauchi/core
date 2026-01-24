//! User Experience storage operations.
//!
//! Handles persistence for aha moments tracking and demo contact state.

use rusqlite::params;

use super::{Storage, StorageError};
use crate::aha_moments::AhaMomentTracker;
use crate::demo_contact::DemoContactState;

impl Storage {
    // === Aha Moments Operations ===

    /// Saves the aha moments tracker state.
    pub fn save_aha_tracker(&self, tracker: &AhaMomentTracker) -> Result<(), StorageError> {
        let json = tracker
            .to_json()
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO ux_state (id, aha_tracker_json, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET aha_tracker_json = ?1, updated_at = ?2",
            params![json, now as i64],
        )?;

        Ok(())
    }

    /// Loads the aha moments tracker state.
    pub fn load_aha_tracker(&self) -> Result<Option<AhaMomentTracker>, StorageError> {
        let result = self.conn.query_row(
            "SELECT aha_tracker_json FROM ux_state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        );

        match result {
            Ok(Some(json)) => {
                let tracker = AhaMomentTracker::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(tracker))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Loads aha tracker or creates a new one if none exists.
    pub fn load_or_create_aha_tracker(&self) -> Result<AhaMomentTracker, StorageError> {
        match self.load_aha_tracker()? {
            Some(tracker) => Ok(tracker),
            None => Ok(AhaMomentTracker::new()),
        }
    }

    // === Demo Contact Operations ===

    /// Saves the demo contact state.
    pub fn save_demo_contact_state(&self, state: &DemoContactState) -> Result<(), StorageError> {
        let json = state
            .to_json()
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO ux_state (id, demo_contact_json, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET demo_contact_json = ?1, updated_at = ?2",
            params![json, now as i64],
        )?;

        Ok(())
    }

    /// Loads the demo contact state.
    pub fn load_demo_contact_state(&self) -> Result<Option<DemoContactState>, StorageError> {
        let result = self.conn.query_row(
            "SELECT demo_contact_json FROM ux_state WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        );

        match result {
            Ok(Some(json)) => {
                let state = DemoContactState::from_json(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(state))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Loads demo contact state or creates a new active one if none exists.
    pub fn load_or_create_demo_contact_state(&self) -> Result<DemoContactState, StorageError> {
        match self.load_demo_contact_state()? {
            Some(state) => Ok(state),
            None => Ok(DemoContactState::new_active()),
        }
    }

    /// Checks if the demo contact is currently active.
    pub fn is_demo_contact_active(&self) -> Result<bool, StorageError> {
        match self.load_demo_contact_state()? {
            Some(state) => Ok(state.is_active),
            None => Ok(false), // Not yet initialized
        }
    }

    // === Combined UX State Operations ===

    /// Saves both aha tracker and demo contact state atomically.
    pub fn save_ux_state(
        &self,
        aha_tracker: &AhaMomentTracker,
        demo_state: &DemoContactState,
    ) -> Result<(), StorageError> {
        let aha_json = aha_tracker
            .to_json()
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        let demo_json = demo_state
            .to_json()
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO ux_state (id, aha_tracker_json, demo_contact_json, updated_at)
             VALUES (1, ?1, ?2, ?3)",
            params![aha_json, demo_json, now as i64],
        )?;

        Ok(())
    }

    /// Loads both aha tracker and demo contact state.
    pub fn load_ux_state(&self) -> Result<(AhaMomentTracker, DemoContactState), StorageError> {
        let aha_tracker = self.load_or_create_aha_tracker()?;
        let demo_state = self.load_or_create_demo_contact_state()?;
        Ok((aha_tracker, demo_state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aha_moments::AhaMomentType;
    use crate::crypto::SymmetricKey;

    fn test_storage() -> Storage {
        Storage::in_memory(SymmetricKey::generate()).unwrap()
    }

    #[test]
    fn test_aha_tracker_save_load() {
        let storage = test_storage();
        let mut tracker = AhaMomentTracker::new();
        tracker.mark_seen(AhaMomentType::CardCreationComplete);
        tracker.mark_seen(AhaMomentType::FirstEdit);

        storage.save_aha_tracker(&tracker).unwrap();
        let loaded = storage.load_aha_tracker().unwrap().unwrap();

        assert!(loaded.has_seen(AhaMomentType::CardCreationComplete));
        assert!(loaded.has_seen(AhaMomentType::FirstEdit));
        assert!(!loaded.has_seen(AhaMomentType::FirstContactAdded));
    }

    #[test]
    fn test_aha_tracker_load_empty() {
        let storage = test_storage();
        let loaded = storage.load_aha_tracker().unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_aha_tracker_load_or_create() {
        let storage = test_storage();

        // First call creates new
        let tracker = storage.load_or_create_aha_tracker().unwrap();
        assert_eq!(tracker.seen_count(), 0);

        // Save it
        let mut tracker = tracker;
        tracker.mark_seen(AhaMomentType::CardCreationComplete);
        storage.save_aha_tracker(&tracker).unwrap();

        // Load again
        let loaded = storage.load_or_create_aha_tracker().unwrap();
        assert!(loaded.has_seen(AhaMomentType::CardCreationComplete));
    }

    #[test]
    fn test_demo_contact_save_load() {
        let storage = test_storage();
        let mut state = DemoContactState::new_active();
        state.advance_to_next_tip();
        state.advance_to_next_tip();

        storage.save_demo_contact_state(&state).unwrap();
        let loaded = storage.load_demo_contact_state().unwrap().unwrap();

        assert!(loaded.is_active);
        assert_eq!(loaded.update_count, 2);
        assert_eq!(loaded.current_tip_index, state.current_tip_index);
    }

    #[test]
    fn test_demo_contact_dismiss_persists() {
        let storage = test_storage();
        let mut state = DemoContactState::new_active();
        state.dismiss();

        storage.save_demo_contact_state(&state).unwrap();
        let loaded = storage.load_demo_contact_state().unwrap().unwrap();

        assert!(!loaded.is_active);
        assert!(loaded.was_dismissed);
    }

    #[test]
    fn test_demo_contact_load_or_create() {
        let storage = test_storage();

        // First call creates active state
        let state = storage.load_or_create_demo_contact_state().unwrap();
        assert!(state.is_active);
    }

    #[test]
    fn test_is_demo_contact_active() {
        let storage = test_storage();

        // Not initialized yet
        assert!(!storage.is_demo_contact_active().unwrap());

        // Save active state
        let state = DemoContactState::new_active();
        storage.save_demo_contact_state(&state).unwrap();
        assert!(storage.is_demo_contact_active().unwrap());

        // Dismiss and save
        let mut state = state;
        state.dismiss();
        storage.save_demo_contact_state(&state).unwrap();
        assert!(!storage.is_demo_contact_active().unwrap());
    }

    #[test]
    fn test_combined_ux_state() {
        let storage = test_storage();

        let mut tracker = AhaMomentTracker::new();
        tracker.mark_seen(AhaMomentType::CardCreationComplete);

        let mut demo_state = DemoContactState::new_active();
        demo_state.advance_to_next_tip();

        storage.save_ux_state(&tracker, &demo_state).unwrap();

        let (loaded_tracker, loaded_demo) = storage.load_ux_state().unwrap();

        assert!(loaded_tracker.has_seen(AhaMomentType::CardCreationComplete));
        assert!(loaded_demo.is_active);
        assert_eq!(loaded_demo.update_count, 1);
    }
}
