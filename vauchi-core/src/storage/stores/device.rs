// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Device domain persistence view.
//!
//! `DeviceStore` owns this device's identity record (`device_info`) and the
//! multi-device registry (`device_registry`). Inter-device *sync* state lives
//! in [`SyncStore`](super::SyncStore) per decision (a) of problem record
//! `2026-06-09-storage-per-domain-store-boundaries`; replay-nonce defense
//! (ADR-029) is still on `Storage` pending its own exchange/security store.

use std::sync::Arc;

use rusqlite::{Connection, params};
use zeroize::Zeroizing;

use super::super::{Storage, StorageError};
use crate::clock::Clock;
use crate::crypto::{PublicKey, SigningKeyPair, SymmetricKey};
use crate::identity::Identity;
use crate::identity::device::{
    BroadcastDevice, DeviceRegistry, MAX_DEVICES, PinnedDeviceKeys, RegistryBroadcast,
};

/// Scoped persistence view for the device domain: this device's info record and
/// the device registry.
pub struct DeviceStore<'a> {
    conn: &'a Connection,
    key: &'a SymmetricKey,
    clock: &'a Arc<dyn Clock>,
    /// A mask profile's identity is derived from the mask seed and has no
    /// identity row of its own (ADR-080).
    identity_is_derived: bool,
}

impl Storage {
    /// Scoped persistence view for the device domain.
    ///
    /// The returned [`DeviceStore`] borrows this storage's connection,
    /// encryption key, and clock; it cannot outlive `self` and reaches only
    /// the device-owned tables.
    pub fn device(&self) -> crate::storage::Sealed<'_, DeviceStore<'_>> {
        self.sealed({
            DeviceStore {
                conn: &self.conn,
                key: &self.encryption_key,
                clock: &self.clock,
                identity_is_derived: self.mask.is_none(),
            }
        })
    }
}

impl DeviceStore<'_> {
    fn now_secs(&self) -> u64 {
        self.clock.unix_seconds()
    }

    /// Saves current device info (encrypted).
    pub fn save_device_info(
        &self,
        device_id: &[u8; 32],
        device_index: u32,
        device_name: &str,
        created_at: u64,
    ) -> Result<(), StorageError> {
        let json = serde_json::json!({
            "device_id": device_id.as_slice(),
            "device_index": device_index,
            "device_name": device_name,
            "created_at": created_at,
        });
        let json_bytes =
            serde_json::to_vec(&json).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted = crate::crypto::encrypt(self.key, &json_bytes)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        self.conn.execute(
            "INSERT OR REPLACE INTO device_info (id, device_info_encrypted) VALUES (1, ?1)",
            params![encrypted],
        )?;
        Ok(())
    }

    /// Loads current device info (decrypted).
    /// Returns (device_id, device_index, device_name, created_at) if found.
    #[allow(clippy::type_complexity)]
    pub fn load_device_info(&self) -> Result<Option<([u8; 32], u32, String, u64)>, StorageError> {
        let result = self.conn.query_row(
            "SELECT device_info_encrypted FROM device_info WHERE id = 1",
            [],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );

        match result {
            Ok(Some(encrypted)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json: serde_json::Value = serde_json::from_slice(&decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let device_id_vec: Vec<u8> = serde_json::from_value(json["device_id"].clone())
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let device_id: [u8; 32] = device_id_vec
                    .try_into()
                    .map_err(|_| StorageError::Encryption("Invalid device ID length".into()))?;
                let device_index = json["device_index"].as_u64().unwrap_or(0) as u32;
                let device_name = json["device_name"].as_str().unwrap_or("").to_string();
                let created_at = json["created_at"].as_u64().unwrap_or(0);
                Ok(Some((device_id, device_index, device_name, created_at)))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Checks if device info exists.
    pub fn has_device_info(&self) -> Result<bool, StorageError> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM device_info WHERE id = 1", [], |row| {
                    row.get(0)
                })?;
        Ok(count > 0)
    }

    /// Deletes this device's info row (used during device reset / unlink).
    pub fn clear_device_info(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM device_info", [])?;
        Ok(())
    }

    /// This device's random signing key for `identity`, generated on first
    /// use (ADR-079).
    ///
    /// Contacts pin the public half and accept no other for this device, so a
    /// stored key that does not open is an error and is never replaced. The
    /// row names the identity it belongs to: a key one identity signed with
    /// must never sign for another, or the two identities are linked
    /// (vauchi/private#970).
    pub fn load_or_create_device_signing_key(
        &self,
        identity: &Identity,
    ) -> Result<SigningKeyPair, StorageError> {
        let owner = identity.signing_public_key();
        if self.stored_device_signing_key()?.is_none() {
            let seed = Zeroizing::new(crate::crypto::random_bytes::<32>());
            self.insert_device_signing_key(owner, &seed)?;
        }
        self.open_device_signing_key(owner)
    }

    /// Keeps the key a joining device generated before it had storage. A
    /// different key already here is refused: contacts accept only one.
    pub fn adopt_device_signing_key(
        &self,
        identity: &Identity,
        seed: &Zeroizing<[u8; 32]>,
    ) -> Result<(), StorageError> {
        let owner = identity.signing_public_key();
        self.insert_device_signing_key(owner, seed)?;
        let kept = self.open_device_signing_key(owner)?;
        if kept.public_key().as_bytes() != SigningKeyPair::from_seed(seed).public_key().as_bytes() {
            return Err(StorageError::InvalidData(
                "a different device signing key is already stored".into(),
            ));
        }
        Ok(())
    }

    /// Stores `seed` for `owner` unless a key is already stored. IGNORE,
    /// then read back: the app and its background worker can both get here,
    /// and only one key may ever exist. The identity check is in the same
    /// statement so a worker that outlives an identity deletion leaves no
    /// key behind (ADR-079, vauchi/private#970).
    fn insert_device_signing_key(
        &self,
        owner: &[u8; 32],
        seed: &Zeroizing<[u8; 32]>,
    ) -> Result<(), StorageError> {
        let sealed = self.seal_device_signing_key(owner, seed)?;
        let holder = if self.identity_is_derived {
            "SELECT 1, ?1"
        } else {
            "SELECT 1, ?1 WHERE EXISTS (SELECT 1 FROM identity WHERE id = 1)"
        };
        self.conn.execute(
            &format!("INSERT OR IGNORE INTO device_signing_key (id, key_encrypted) {holder}"),
            params![sealed],
        )?;
        Ok(())
    }

    fn seal_device_signing_key(
        &self,
        owner: &[u8; 32],
        seed: &Zeroizing<[u8; 32]>,
    ) -> Result<Vec<u8>, StorageError> {
        let mut plaintext = Zeroizing::new(Vec::with_capacity(64));
        plaintext.extend_from_slice(owner);
        plaintext.extend_from_slice(seed.as_slice());
        crate::crypto::encrypt(self.key, plaintext.as_slice())
            .map_err(|e| StorageError::Encryption(e.to_string()))
    }

    /// The stored key, if it belongs to `owner`. A row from before keys
    /// named their identity is the current identity's (its contacts pinned
    /// it) and is rewritten to name it.
    fn open_device_signing_key(&self, owner: &[u8; 32]) -> Result<SigningKeyPair, StorageError> {
        self.open_device_signing_key_once(owner, true)
    }

    fn open_device_signing_key_once(
        &self,
        owner: &[u8; 32],
        may_name_owner: bool,
    ) -> Result<SigningKeyPair, StorageError> {
        let stored = self.stored_device_signing_key()?.ok_or_else(|| {
            StorageError::InvalidData(
                "a device signing key is created only while an identity is stored".into(),
            )
        })?;
        let opened = Zeroizing::new(
            crate::crypto::decrypt(self.key, &stored)
                .map_err(|e| StorageError::Encryption(e.to_string()))?,
        );
        let (named_owner, seed_bytes) = match opened.len() {
            64 => (Some(&opened[..32]), &opened[32..]),
            32 => (None, &opened[..]),
            _ => {
                return Err(StorageError::InvalidData(
                    "device signing key has invalid length".into(),
                ));
            }
        };
        let seed: Zeroizing<[u8; 32]> = Zeroizing::new(seed_bytes.try_into().map_err(|_| {
            StorageError::InvalidData("device signing key has invalid length".into())
        })?);
        match named_owner {
            Some(named) if named == owner.as_slice() => {}
            Some(_) => {
                return Err(StorageError::InvalidData(
                    "the stored device signing key belongs to another identity".into(),
                ));
            }
            None if may_name_owner => {
                let sealed = self.seal_device_signing_key(owner, &seed)?;
                self.conn.execute(
                    "UPDATE device_signing_key SET key_encrypted = ?1 WHERE id = 1 AND key_encrypted = ?2",
                    params![sealed, stored],
                )?;
                // Read back: another connection may have named a different
                // identity first, and then this one must not sign with it.
                return self.open_device_signing_key_once(owner, false);
            }
            None => {
                return Err(StorageError::InvalidData(
                    "the stored device signing key names no identity".into(),
                ));
            }
        }
        Ok(SigningKeyPair::from_seed(&seed))
    }

    /// The saved registry, or a new one naming this device and its key. Takes
    /// the identity so the key is only ever created while one is loaded.
    pub fn own_device_registry(&self, identity: &Identity) -> Result<DeviceRegistry, StorageError> {
        let device_key = self.load_or_create_device_signing_key(identity)?;
        let device_public_key = *device_key.public_key().as_bytes();
        let Some(mut saved) = self.load_own_device_registry(identity)? else {
            return Ok(identity.initial_device_registry_naming(device_public_key));
        };
        // A registry saved before this device had a key (a link from an
        // older host, hand-back recovery) names it here, once. A registry
        // whose entry for this device names another key is a database
        // restored onto a device with a fresh key: signing as that entry
        // would attribute this device's broadcasts to a key it does not
        // hold, so it is refused until the device takes a new index.
        let changed = saved
            .name_device_key(
                identity.device_id(),
                device_public_key,
                identity.signing_keypair(),
            )
            .map_err(|error| match error {
                crate::identity::device::DeviceError::DeviceKeyChanged => {
                    StorageError::InvalidData(
                        "this device's registry entry names another signing key".into(),
                    )
                }
                crate::identity::device::DeviceError::DeviceNotFound => {
                    StorageError::InvalidData("this device is not in its own saved registry".into())
                }
                other => StorageError::InvalidData(other.to_string()),
            })?;
        if changed {
            self.save_device_registry(&saved)?;
        }
        Ok(saved)
    }

    fn stored_device_signing_key(&self) -> Result<Option<Vec<u8>>, StorageError> {
        match self.conn.query_row(
            "SELECT key_encrypted FROM device_signing_key WHERE id = 1",
            [],
            |row| row.get(0),
        ) {
            Ok(stored) => Ok(Some(stored)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Forgets this device's signing key.
    pub fn clear_device_signing_key(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM device_signing_key", [])?;
        Ok(())
    }

    /// The saved registry, refused when `identity` did not sign it. Every
    /// registry this install saves is signed by its identity; one that is
    /// not was left by an identity this install held before, and changing
    /// it would re-sign another identity's devices as this one's.
    pub fn load_own_device_registry(
        &self,
        identity: &Identity,
    ) -> Result<Option<DeviceRegistry>, StorageError> {
        let Some(saved) = self.load_device_registry()? else {
            return Ok(None);
        };
        if !saved.verify(&identity.signing_keypair().public_key()) {
            return Err(StorageError::InvalidData(
                "the saved device registry belongs to another identity".into(),
            ));
        }
        Ok(Some(saved))
    }

    /// The saved registry if `identity` signed it, and none otherwise: for
    /// reads that only list, count or sync to this identity's devices. A
    /// registry another identity left must not name sync targets or linked
    /// devices, and must not outrank this identity's next registry by its
    /// version (private#970). Paths that re-sign use
    /// [`Self::load_own_device_registry`], which refuses it instead.
    pub fn load_device_registry_if_own(
        &self,
        identity: &Identity,
    ) -> Result<Option<DeviceRegistry>, StorageError> {
        let owner = identity.signing_keypair().public_key();
        let Some(saved) = self.load_device_registry()? else {
            return Ok(None);
        };
        if !saved.verify(&owner) {
            tracing::warn!("[DeviceStore] ignored a saved device registry another identity signed");
            return Ok(None);
        }
        Ok(Some(saved))
    }

    /// Forgets this install's device registry.
    pub fn clear_device_registry(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM device_registry", [])?;
        Ok(())
    }

    /// Saves the device registry (encrypted).
    pub fn save_device_registry(&self, registry: &DeviceRegistry) -> Result<(), StorageError> {
        let registry_json = serde_json::to_string(registry)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;

        let registry_encrypted = crate::crypto::encrypt(self.key, registry_json.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = self.now_secs();

        self.conn.execute(
            "INSERT OR REPLACE INTO device_registry (id, registry_json, registry_json_encrypted, version, updated_at)
             VALUES (1, '', ?1, ?2, ?3)",
            params![registry_encrypted, registry.version() as i64, now as i64,],
        )?;
        Ok(())
    }

    /// Loads the device registry (decrypted).
    pub fn load_device_registry(&self) -> Result<Option<DeviceRegistry>, StorageError> {
        let result = self.conn.query_row(
            "SELECT registry_json_encrypted, registry_json FROM device_registry WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: String = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                let registry: DeviceRegistry = serde_json::from_str(&json)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(registry))
            }
            Ok((_, plaintext)) if !plaintext.is_empty() => {
                let registry: DeviceRegistry = serde_json::from_str(&plaintext)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(registry))
            }
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Loads the raw device registry JSON string.
    ///
    /// Used by GDPR export to include device information without
    /// requiring knowledge of the DeviceRegistry struct.
    pub fn load_device_registry_json(&self) -> Result<Option<String>, StorageError> {
        let result = self.conn.query_row(
            "SELECT registry_json_encrypted, registry_json FROM device_registry WHERE id = 1",
            [],
            |row| {
                let encrypted: Option<Vec<u8>> = row.get(0)?;
                let plaintext: String = row.get(1)?;
                Ok((encrypted, plaintext))
            },
        );

        match result {
            Ok((Some(encrypted), _)) if !encrypted.is_empty() => {
                let decrypted = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|e| StorageError::Encryption(e.to_string()))?;
                let json = String::from_utf8(decrypted)
                    .map_err(|e| StorageError::Serialization(e.to_string()))?;
                Ok(Some(json))
            }
            Ok((_, plaintext)) if !plaintext.is_empty() => Ok(Some(plaintext)),
            Ok(_) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    /// Checks if device registry exists.
    pub fn has_device_registry(&self) -> Result<bool, StorageError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM device_registry WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Verifies and atomically retains a contact's newest signed device registry.
    ///
    /// Invalid signatures, replayed versions, stale timestamps, and excessive
    /// future clock skew are rejected without replacing the last trusted value.
    pub fn save_contact_device_registry(
        &self,
        contact_id: &str,
        broadcast: &RegistryBroadcast,
        contact_signing_key: &[u8; 32],
        max_age_secs: u64,
    ) -> Result<(), StorageError> {
        if broadcast.active_device_count() == 0 || broadcast.active_device_count() > MAX_DEVICES {
            return Err(StorageError::InvalidData(format!(
                "contact device registry must contain 1..={MAX_DEVICES} active devices"
            )));
        }
        let mut unique_ids = std::collections::HashSet::new();
        if !broadcast
            .active_devices()
            .iter()
            .all(|device| unique_ids.insert(device.device_id))
        {
            return Err(StorageError::InvalidData(
                "contact device registry contains duplicate device IDs".into(),
            ));
        }
        let stored_version = i64::try_from(broadcast.version()).map_err(|_| {
            StorageError::InvalidData(
                "contact device registry version exceeds storage range".into(),
            )
        })?;
        let previously_active = self.load_contact_active_devices(contact_id)?;
        let last_stored_version = self
            .conn
            .query_row(
                "SELECT version FROM contact_device_registries WHERE contact_id = ?1",
                params![contact_id],
                |row| row.get::<_, i64>(0),
            )
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(0),
                other => Err(other),
            })?;
        let last_version = u64::try_from(last_stored_version).map_err(|_| {
            StorageError::InvalidData("stored contact device registry version is negative".into())
        })?;

        let contact_identity = PublicKey::from_bytes(*contact_signing_key);
        if self.pin_a_re_sent_held_version(
            contact_id,
            broadcast,
            &contact_identity,
            last_version,
            max_age_secs,
        )? {
            return Ok(());
        }
        broadcast
            .verify_with_freshness(
                &contact_identity,
                last_version,
                self.now_secs(),
                max_age_secs,
            )
            .map_err(|error| StorageError::InvalidData(error.to_string()))?;
        // SC-62: the broadcast is accepted per device. The signer must be an
        // active device of this broadcast and have signed it with its own
        // key, and no key a device showed before may change under its id
        // (vauchi/private#970).
        refuse_unusable_signing_keys(broadcast)?;
        let mut pins = self
            .load_contact_device_pins(contact_id)?
            .unwrap_or_default();
        pins.accept(&contact_identity, broadcast)
            .map_err(|error| StorageError::InvalidData(error.to_string()))?;

        let json = serde_json::to_vec(broadcast)
            .map_err(|error| StorageError::Serialization(error.to_string()))?;
        let encrypted = crate::crypto::encrypt(self.key, &json)
            .map_err(|error| StorageError::Encryption(error.to_string()))?;
        self.conn
            .execute_batch("SAVEPOINT save_contact_device_registry")?;
        let mutation_result = (|| -> Result<(), StorageError> {
            self.conn.execute(
                "INSERT INTO contact_device_registries (contact_id, broadcast_encrypted, version, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(contact_id) DO UPDATE SET
                    broadcast_encrypted = excluded.broadcast_encrypted,
                    version = excluded.version,
                    updated_at = excluded.updated_at",
                params![contact_id, encrypted, stored_version, self.now_secs() as i64],
            )?;
            self.save_contact_device_pins(contact_id, &pins)?;
            for removed in previously_active.iter().filter(|old| {
                !broadcast
                    .active_devices()
                    .iter()
                    .any(|current| current.device_id == old.device_id)
            }) {
                self.conn.execute(
                    "DELETE FROM contact_ratchets WHERE contact_id = ?1 AND peer_device_id = ?2",
                    params![contact_id, removed.device_id.as_slice()],
                )?;
            }
            Ok(())
        })();
        if let Err(error) = mutation_result {
            self.conn.execute_batch(
                "ROLLBACK TO save_contact_device_registry;
                 RELEASE save_contact_device_registry;",
            )?;
            return Err(error);
        }
        self.conn
            .execute_batch("RELEASE save_contact_device_registry")?;
        Ok(())
    }

    /// Pins the keys of a broadcast at the version already held, when no
    /// pins are held for the contact, and reports whether it did. A database
    /// upgraded from before pins holds the registry, and the contact's
    /// handshake only re-sends that version, which the version check refuses
    /// before anything is pinned (private#970). Only a broadcast naming the
    /// held devices and keys pins, verified as any other; the registry
    /// itself stays as held.
    fn pin_a_re_sent_held_version(
        &self,
        contact_id: &str,
        broadcast: &RegistryBroadcast,
        contact_identity: &PublicKey,
        last_version: u64,
        max_age_secs: u64,
    ) -> Result<bool, StorageError> {
        if last_version == 0 || broadcast.version() != last_version {
            return Ok(false);
        }
        if self
            .load_contact_device_pins(contact_id)?
            .is_some_and(|pins| pins.pairs().next().is_some())
        {
            return Ok(false);
        }
        let Some(held) = self.load_contact_device_registry(contact_id)? else {
            return Ok(false);
        };
        let devices = |registry: &RegistryBroadcast| {
            registry
                .active_devices()
                .iter()
                .map(|device| (device.device_id, device.signing_public_key))
                .collect::<Vec<_>>()
        };
        if devices(&held) != devices(broadcast) {
            return Ok(false);
        }
        broadcast
            .verify_with_freshness(
                contact_identity,
                last_version - 1,
                self.now_secs(),
                max_age_secs,
            )
            .map_err(|error| StorageError::InvalidData(error.to_string()))?;
        refuse_unusable_signing_keys(broadcast)?;
        let mut pins = PinnedDeviceKeys::default();
        pins.accept(contact_identity, broadcast)
            .map_err(|error| StorageError::InvalidData(error.to_string()))?;
        self.save_contact_device_pins(contact_id, &pins)?;
        Ok(true)
    }

    /// The signing keys this contact's devices showed first, or `None`
    /// before any device-signed broadcast was accepted from it.
    pub fn load_contact_device_pins(
        &self,
        contact_id: &str,
    ) -> Result<Option<PinnedDeviceKeys>, StorageError> {
        let sealed: Option<Vec<u8>> = match self.conn.query_row(
            "SELECT pins_encrypted FROM contact_device_key_pins WHERE contact_id = ?1",
            params![contact_id],
            |row| row.get(0),
        ) {
            Ok(sealed) => Some(sealed),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(StorageError::Database(e)),
        };
        let Some(sealed) = sealed else {
            return Ok(None);
        };
        let json = crate::crypto::decrypt(self.key, &sealed)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        let pairs: Vec<([u8; 32], [u8; 32])> = serde_json::from_slice(&json)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        Ok(Some(PinnedDeviceKeys::from_pairs(pairs)))
    }

    /// Replaces the contact's pins. Callers only ever grow them through
    /// `PinnedDeviceKeys::accept`, so a replacement never forgets a key.
    pub fn save_contact_device_pins(
        &self,
        contact_id: &str,
        pins: &PinnedDeviceKeys,
    ) -> Result<(), StorageError> {
        let pairs: Vec<([u8; 32], [u8; 32])> = pins.pairs().collect();
        let json =
            serde_json::to_vec(&pairs).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let sealed = crate::crypto::encrypt(self.key, &json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO contact_device_key_pins (contact_id, pins_encrypted, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(contact_id) DO UPDATE SET
                pins_encrypted = excluded.pins_encrypted,
                updated_at = excluded.updated_at",
            params![contact_id, sealed, self.now_secs() as i64],
        )?;
        Ok(())
    }

    /// Loads the last verified signed registry retained for a contact.
    pub fn load_contact_device_registry(
        &self,
        contact_id: &str,
    ) -> Result<Option<RegistryBroadcast>, StorageError> {
        let result = self.conn.query_row(
            "SELECT broadcast_encrypted FROM contact_device_registries WHERE contact_id = ?1",
            params![contact_id],
            |row| row.get::<_, Vec<u8>>(0),
        );
        match result {
            Ok(encrypted) => {
                let json = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|error| StorageError::Encryption(error.to_string()))?;
                serde_json::from_slice(&json)
                    .map(Some)
                    .map_err(|error| StorageError::Serialization(error.to_string()))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(StorageError::Database(error)),
        }
    }

    /// Returns the contact devices that core currently authorizes as targets.
    pub fn load_contact_active_devices(
        &self,
        contact_id: &str,
    ) -> Result<Vec<BroadcastDevice>, StorageError> {
        Ok(self
            .load_contact_device_registry(contact_id)?
            .map(|registry| registry.active_devices().to_vec())
            .unwrap_or_default())
    }

    /// Lists all verified contact registries for encrypted owner-device sync.
    pub fn list_contact_device_registries(
        &self,
    ) -> Result<Vec<(String, RegistryBroadcast)>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT contact_id, broadcast_encrypted FROM contact_device_registries ORDER BY contact_id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(contact_id, encrypted)| {
                let json = crate::crypto::decrypt(self.key, &encrypted)
                    .map_err(|error| StorageError::Encryption(error.to_string()))?;
                let registry = serde_json::from_slice(&json)
                    .map_err(|error| StorageError::Serialization(error.to_string()))?;
                Ok((contact_id, registry))
            })
            .collect()
    }
}

/// A key is checked before it is pinned: the link checks keys on the way
/// in, a contact's broadcast has not been through that.
fn refuse_unusable_signing_keys(broadcast: &RegistryBroadcast) -> Result<(), StorageError> {
    if broadcast
        .active_devices()
        .iter()
        .filter_map(|device| device.signing_public_key)
        .any(|key| !crate::crypto::signing::is_usable_public_key(&key))
    {
        return Err(StorageError::InvalidData(
            "contact device registry names an unusable signing key".into(),
        ));
    }
    Ok(())
}
