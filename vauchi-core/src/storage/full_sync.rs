// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use rusqlite::Connection;

use super::StorageError;

/// Raises the connection to at least FULL synchronous durability while it
/// lives, for writes a power loss must not undo, and restores the previous
/// policy when dropped.
pub(crate) struct FullSync<'a> {
    conn: &'a Connection,
    previous: i64,
}

impl<'a> FullSync<'a> {
    pub(crate) fn full(conn: &'a Connection) -> Result<Self, StorageError> {
        let previous: i64 = conn.pragma_query_value(None, "synchronous", |row| row.get(0))?;
        conn.pragma_update(None, "synchronous", previous.max(2))?;
        Ok(Self { conn, previous })
    }
}

impl Drop for FullSync<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.conn.pragma_update(None, "synchronous", self.previous) {
            // #1178: restoration cannot turn an already committed write into a failure.
            tracing::warn!(target: "vauchi.storage",
                sqlite_error_code = ?error.sqlite_error().map(|e| e.extended_code),
                "could not restore storage sync policy after a durable write");
        }
    }
}

#[cfg(all(test, feature = "notice-core"))]
mod tests {
    use super::*;
    use crate::crypto::SymmetricKey;
    use crate::storage::{NoticeStateKind, Storage};

    fn storage() -> Storage {
        Storage::in_memory(SymmetricKey::generate()).unwrap()
    }

    fn save(storage: &Storage, kind: NoticeStateKind) -> Result<(), StorageError> {
        storage.notices().save("contact", kind, &[1; 32], b"state")
    }

    // @internal
    #[test]
    fn a_notice_transaction_commits_all_states_at_full_sync() {
        let storage = storage();
        storage
            .conn
            .execute_batch(
                "CREATE TABLE observed(level INTEGER);
            CREATE TRIGGER observe_notice AFTER INSERT ON notice_states BEGIN
                INSERT INTO observed SELECT synchronous FROM pragma_synchronous;
            END;",
            )
            .unwrap();
        let receipt = storage
            .with_notice_transaction(|| {
                save(&storage, NoticeStateKind::Sender)?;
                save(&storage, NoticeStateKind::Receiver)?;
                Ok::<_, StorageError>("saved")
            })
            .unwrap();
        assert_eq!(receipt, "saved");
        let minimum: i64 = storage
            .conn
            .query_row("SELECT min(level) FROM observed", [], |row| row.get(0))
            .unwrap();
        assert!(minimum >= 2);
        assert_eq!(
            storage
                .conn
                .pragma_query_value::<i64, _>(None, "synchronous", |row| row.get(0))
                .unwrap(),
            1
        );
        assert_eq!(
            storage
                .notices()
                .load("contact", NoticeStateKind::Sender, &[1; 32])
                .unwrap()
                .as_deref()
                .map(Vec::as_slice),
            Some(b"state".as_slice())
        );
        assert_eq!(
            storage
                .notices()
                .load("contact", NoticeStateKind::Receiver, &[1; 32])
                .unwrap()
                .as_deref()
                .map(Vec::as_slice),
            Some(b"state".as_slice())
        );
    }

    // @internal
    #[test]
    fn a_failed_notice_commit_releases_nothing_and_rolls_back_every_state() {
        let storage = storage();
        storage.arm_commit_fault();
        let failed = storage.with_notice_transaction(|| {
            save(&storage, NoticeStateKind::Sender)?;
            save(&storage, NoticeStateKind::Receiver)?;
            Ok::<_, StorageError>("must not leave")
        });
        assert!(matches!(failed, Err(StorageError::Database(_))));
        assert_eq!(
            storage
                .notices()
                .load("contact", NoticeStateKind::Sender, &[1; 32])
                .unwrap(),
            None
        );
        assert_eq!(
            storage
                .notices()
                .load("contact", NoticeStateKind::Receiver, &[1; 32])
                .unwrap(),
            None
        );
        storage
            .with_notice_transaction(|| save(&storage, NoticeStateKind::Sender))
            .unwrap();
        assert_eq!(
            storage
                .notices()
                .load("contact", NoticeStateKind::Sender, &[1; 32])
                .unwrap()
                .as_deref()
                .map(Vec::as_slice),
            Some(b"state".as_slice())
        );
    }

    // @internal
    #[test]
    fn a_notice_receipt_never_escapes_an_uncommitted_outer_transaction() {
        let storage = storage();
        storage.begin_transaction().unwrap();
        let failed = storage.with_notice_transaction(|| {
            save(&storage, NoticeStateKind::Sender)?;
            Ok::<_, StorageError>("must not leave")
        });
        assert!(matches!(failed, Err(StorageError::InvalidData(_))));
        storage.rollback();
        assert_eq!(
            storage
                .notices()
                .load("contact", NoticeStateKind::Sender, &[1; 32])
                .unwrap(),
            None
        );
    }
}
