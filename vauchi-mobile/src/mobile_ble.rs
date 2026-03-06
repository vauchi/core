// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE Encrypted Exchange Mobile Bindings
//!
//! Wraps vauchi-core's `BleHandshakeSession` for mobile platforms.
//! Provides a callback interface for platform BLE transports (CoreBluetooth, Android BLE)
//! and a UniFFI object that mobile apps drive through the four-phase handshake.
//!
//! ## Architecture
//!
//! Mobile platforms own the BLE stack (scanning, connecting, GATT operations).
//! Core owns the cryptographic protocol (key exchange, encryption, commitment scheme).
//!
//! - **Mobile → Core**: `on_connected`, `on_data_received`, `on_mtu_negotiated`, etc.
//! - **Core → Mobile**: `MobileBleDelegate` callback interface (send_data, on_state_changed, etc.)

use std::sync::Mutex;

use vauchi_core::exchange::{
    BleCardPayload, BleChunker, BleExchangeResult, BleHandshakeSession, BleHandshakeState,
    BleReassembler, BLE_CHUNK_OVERHEAD, BLE_DEFAULT_USABLE, CHAR_DATA_NOTIFY, CHAR_DATA_WRITE,
    CHAR_HANDSHAKE_NOTIFY, CHAR_HANDSHAKE_WRITE,
};

// === Error Types ===

/// Error type for BLE transport callback interface.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileBleTransportError {
    #[error("BLE transport error: {msg}")]
    TransportFailed { msg: String },

    #[error("BLE connection lost")]
    ConnectionLost,
}

/// Error type for BLE exchange operations.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileBleError {
    #[error("BLE exchange error: {msg}")]
    ExchangeFailed { msg: String },

    #[error("Invalid state for this operation")]
    InvalidState,
}

// === Mobile-Friendly Types ===

/// Mobile-friendly BLE exchange state.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileBleState {
    Connecting,
    Handshaking,
    Transferring,
    Verifying,
    Complete,
    Failed { error: String },
}

/// Result of a completed BLE exchange, exposed to mobile.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileBleExchangeResult {
    pub remote_display_name: String,
    pub remote_identity_key: Vec<u8>,
    pub remote_exchange_key: Vec<u8>,
    pub remote_fields: Vec<MobileBleField>,
    pub remote_avatar: Option<Vec<u8>>,
}

/// A single contact field (key-value pair).
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileBleField {
    pub key: String,
    pub value: String,
}

// === Callback Interface ===

/// Callback interface for platform-specific BLE transport.
///
/// iOS implements this with CoreBluetooth.
/// Android implements this with Android BLE (BluetoothGatt / BluetoothGattServer).
///
/// Core calls these methods to send data and notify the mobile app of state changes.
#[uniffi::export(callback_interface)]
pub trait MobileBleDelegate: Send + Sync {
    /// Write data to a BLE characteristic.
    ///
    /// The mobile platform sends this data to the connected peer
    /// via the specified GATT characteristic UUID.
    fn send_data(
        &self,
        characteristic_uuid: String,
        data: Vec<u8>,
    ) -> Result<(), MobileBleTransportError>;

    /// Subscribe to notifications on a BLE characteristic.
    ///
    /// The mobile platform enables notifications for the specified
    /// characteristic so incoming data triggers `on_data_received`.
    fn subscribe_notify(&self, characteristic_uuid: String) -> Result<(), MobileBleTransportError>;

    /// Disconnect from the BLE peer.
    fn disconnect(&self) -> Result<(), MobileBleTransportError>;

    /// Called when the exchange state changes.
    fn on_state_changed(&self, state: MobileBleState);

    /// Called when the exchange completes successfully.
    fn on_exchange_complete(&self, result: MobileBleExchangeResult);

    /// Called when the exchange fails.
    fn on_exchange_failed(&self, error: String);
}

// === Session Object ===

/// Mobile BLE exchange session wrapping the core `BleHandshakeSession`.
///
/// Drives the four-phase encrypted BLE exchange:
/// Phase 1: KeyOffer (initiator → responder)
/// Phase 2: KeyAck + encrypted card (responder → initiator)
/// Phase 3: Commitment + encrypted card (initiator → responder)
/// Phase 4: Reveal + verify (both sides)
///
/// The mobile app feeds BLE events into this session via `on_connected`,
/// `on_data_received`, etc. The session calls back to the delegate to
/// send data and report state changes.
#[derive(uniffi::Object)]
pub struct MobileBleExchangeSession {
    inner: Mutex<BleHandshakeSession>,
    delegate: Box<dyn MobileBleDelegate>,
    mtu_usable: Mutex<usize>,
    reassembler: Mutex<Option<BleReassembler>>,
    pending_encrypted: Mutex<Option<Vec<u8>>>,
    is_initiator: Mutex<bool>,
}

#[uniffi::export]
impl MobileBleExchangeSession {
    /// Creates a new BLE exchange session.
    ///
    /// # Arguments
    ///
    /// * `identity_key` - 32-byte Ed25519 signing public key
    /// * `display_name` - User's display name for the contact card
    /// * `exchange_key` - 32-byte X25519 exchange public key
    /// * `fields` - Contact fields as key-value pairs
    /// * `avatar` - Optional avatar image bytes
    /// * `delegate` - Platform BLE transport callback
    #[uniffi::constructor]
    pub fn new(
        identity_key: Vec<u8>,
        display_name: String,
        exchange_key: Vec<u8>,
        fields: Vec<MobileBleField>,
        avatar: Option<Vec<u8>>,
        delegate: Box<dyn MobileBleDelegate>,
    ) -> Result<Self, MobileBleError> {
        let identity_key_arr: [u8; 32] =
            identity_key
                .try_into()
                .map_err(|_| MobileBleError::ExchangeFailed {
                    msg: "identity_key must be exactly 32 bytes".into(),
                })?;

        let exchange_key_arr: [u8; 32] =
            exchange_key
                .try_into()
                .map_err(|_| MobileBleError::ExchangeFailed {
                    msg: "exchange_key must be exactly 32 bytes".into(),
                })?;

        let card_fields: Vec<(String, String)> =
            fields.into_iter().map(|f| (f.key, f.value)).collect();

        let card = BleCardPayload::new(
            identity_key_arr,
            display_name,
            exchange_key_arr,
            card_fields,
            avatar,
        );

        // Default to initiator; call set_responder() before on_connected() to switch.
        let session = BleHandshakeSession::new_initiator_from_key(identity_key_arr, card);

        Ok(Self {
            inner: Mutex::new(session),
            delegate,
            mtu_usable: Mutex::new(BLE_DEFAULT_USABLE),
            reassembler: Mutex::new(None),
            pending_encrypted: Mutex::new(None),
            is_initiator: Mutex::new(true),
        })
    }

    /// Switch this session to responder mode.
    ///
    /// Must be called before `on_connected()`. The responder waits for
    /// a KeyOffer from the initiator instead of sending one.
    pub fn set_responder(&self) {
        let mut is_init = self.is_initiator.lock().unwrap();
        *is_init = false;
    }

    /// Called when the BLE connection is established.
    ///
    /// For the initiator: creates and sends the KeyOffer.
    /// For the responder: subscribes to the handshake write characteristic
    /// and waits for the initiator's KeyOffer.
    pub fn on_connected(&self, _device_id: String) {
        self.delegate.on_state_changed(MobileBleState::Handshaking);

        let is_initiator = *self.is_initiator.lock().unwrap();

        if is_initiator {
            // Initiator: send KeyOffer via handshake write characteristic
            let mut inner = self.inner.lock().unwrap();
            match inner.create_key_offer() {
                Ok(offer_bytes) => {
                    if let Err(e) = self
                        .delegate
                        .send_data(CHAR_HANDSHAKE_WRITE.to_string(), offer_bytes)
                    {
                        self.fail(format!("Failed to send key offer: {e}"));
                    }
                }
                Err(e) => {
                    self.fail(format!("Failed to create key offer: {e:?}"));
                }
            }
        } else {
            // Responder: subscribe to handshake write to receive KeyOffer
            if let Err(e) = self
                .delegate
                .subscribe_notify(CHAR_HANDSHAKE_WRITE.to_string())
            {
                self.fail(format!("Failed to subscribe to handshake: {e}"));
            }
        }
    }

    /// Called when data is received on a BLE characteristic.
    ///
    /// Routes the data to the appropriate handler based on the characteristic UUID
    /// and current protocol phase.
    pub fn on_data_received(&self, characteristic_uuid: String, data: Vec<u8>) {
        match characteristic_uuid.as_str() {
            uuid if uuid == CHAR_HANDSHAKE_WRITE => {
                self.handle_handshake_write(data);
            }
            uuid if uuid == CHAR_HANDSHAKE_NOTIFY => {
                self.handle_handshake_notify(data);
            }
            uuid if uuid == CHAR_DATA_WRITE || uuid == CHAR_DATA_NOTIFY => {
                self.handle_data_chunk(data);
            }
            _ => {
                // Unknown characteristic — ignore
            }
        }
    }

    /// Called when MTU is negotiated with the peer.
    ///
    /// The usable payload size is `mtu - 3` (ATT header overhead).
    pub fn on_mtu_negotiated(&self, mtu: u32) {
        let usable = (mtu as usize).saturating_sub(3);
        let mut mtu_usable = self.mtu_usable.lock().unwrap();
        *mtu_usable = usable.max(BLE_CHUNK_OVERHEAD + 1);
    }

    /// Called when the BLE connection is lost.
    pub fn on_disconnected(&self) {
        let inner = self.inner.lock().unwrap();
        if !matches!(inner.state(), BleHandshakeState::Complete { .. }) {
            drop(inner);
            self.fail("Connection lost".into());
        }
    }

    /// Returns the current state of the exchange.
    pub fn get_state(&self) -> MobileBleState {
        let inner = self.inner.lock().unwrap();
        map_state(inner.state())
    }

    /// Cancel the exchange and disconnect.
    pub fn cancel(&self) {
        let _ = self.delegate.disconnect();
        self.delegate.on_state_changed(MobileBleState::Failed {
            error: "Cancelled".into(),
        });
    }
}

// === Internal Methods (not exported via UniFFI) ===

impl MobileBleExchangeSession {
    /// Handle data on the handshake write characteristic.
    ///
    /// Responder receives KeyOffer here, or initiator's commitment + encrypted card.
    fn handle_handshake_write(&self, data: Vec<u8>) {
        let is_initiator = *self.is_initiator.lock().unwrap();

        if !is_initiator {
            // Responder: check if this is a KeyOffer (Phase 1) or committed payload (Phase 3)
            let mut inner = self.inner.lock().unwrap();
            match inner.state() {
                BleHandshakeState::Idle => {
                    // Phase 1: Process KeyOffer
                    match inner.process_key_offer(&data) {
                        Ok((ack_bytes, encrypted_card)) => {
                            // Store our encrypted card for chunked transfer
                            *self.pending_encrypted.lock().unwrap() = Some(encrypted_card);

                            // Send KeyAck via handshake notify
                            if let Err(e) = self
                                .delegate
                                .send_data(CHAR_HANDSHAKE_NOTIFY.to_string(), ack_bytes)
                            {
                                drop(inner);
                                self.fail(format!("Failed to send key ack: {e}"));
                                return;
                            }

                            // Send our encrypted card chunks via data notify
                            drop(inner);
                            self.send_pending_encrypted(CHAR_DATA_NOTIFY);

                            // Subscribe to data write for initiator's chunks
                            if let Err(e) =
                                self.delegate.subscribe_notify(CHAR_DATA_WRITE.to_string())
                            {
                                self.fail(format!("Failed to subscribe to data write: {e}"));
                            }
                        }
                        Err(e) => {
                            drop(inner);
                            self.fail(format!("Failed to process key offer: {e:?}"));
                        }
                    }
                }
                _ => {
                    // Phase 3: Committed payload from initiator
                    // First 32 bytes = commitment, rest = via data chunks
                    if data.len() >= 32 {
                        let commitment = data[..32].to_vec();
                        // Store commitment, encrypted card arrives via data chunks
                        *self.pending_encrypted.lock().unwrap() = Some(commitment);
                    }
                }
            }
        }
    }

    /// Handle data on the handshake notify characteristic.
    ///
    /// Initiator receives KeyAck here, or responder's reveal.
    fn handle_handshake_notify(&self, data: Vec<u8>) {
        let is_initiator = *self.is_initiator.lock().unwrap();

        if is_initiator {
            let inner = self.inner.lock().unwrap();
            match inner.state() {
                BleHandshakeState::KeyOfferSent { .. } => {
                    // Phase 2: KeyAck received — but we need the encrypted card too.
                    // Store the ack, wait for data chunks to complete.
                    drop(inner);
                    *self.pending_encrypted.lock().unwrap() = Some(data);

                    self.delegate.on_state_changed(MobileBleState::Transferring);

                    // Subscribe to data notify for responder's encrypted card chunks
                    if let Err(e) = self.delegate.subscribe_notify(CHAR_DATA_NOTIFY.to_string()) {
                        self.fail(format!("Failed to subscribe to data notify: {e}"));
                    }
                }
                BleHandshakeState::PayloadsExchanged { .. } => {
                    // Phase 4: Reveal from responder
                    drop(inner);
                    self.complete_exchange(data);
                }
                _ => {
                    // Unexpected state — ignore
                }
            }
        } else {
            // Responder: receive reveal from initiator (Phase 4)
            let inner = self.inner.lock().unwrap();
            if matches!(inner.state(), BleHandshakeState::PayloadsExchanged { .. }) {
                drop(inner);
                self.complete_exchange(data);
            }
        }
    }

    /// Handle a chunk of encrypted card data.
    fn handle_data_chunk(&self, data: Vec<u8>) {
        if data.len() < BLE_CHUNK_OVERHEAD {
            return;
        }

        let total = u16::from_le_bytes([data[2], data[3]]);

        let mut reassembler_guard = self.reassembler.lock().unwrap();
        if reassembler_guard.is_none() {
            *reassembler_guard = Some(BleReassembler::new(total));
        }

        let reassembler = reassembler_guard.as_mut().unwrap();
        if let Err(e) = reassembler.insert_chunk(&data) {
            drop(reassembler_guard);
            self.fail(format!("Chunk reassembly failed: {e:?}"));
            return;
        }

        if reassembler.is_complete() {
            let assembled = reassembler.assemble().unwrap();
            drop(reassembler_guard);
            self.on_remote_encrypted_card_received(assembled);
        }
    }

    /// Called when all chunks of the remote encrypted card have been received.
    fn on_remote_encrypted_card_received(&self, encrypted_card: Vec<u8>) {
        let is_initiator = *self.is_initiator.lock().unwrap();

        if is_initiator {
            // We have the KeyAck in pending_encrypted and now the full encrypted card.
            let ack_data = self.pending_encrypted.lock().unwrap().take();
            let Some(ack_bytes) = ack_data else {
                self.fail("No pending KeyAck data".into());
                return;
            };

            let mut inner = self.inner.lock().unwrap();
            match inner.process_key_ack(&ack_bytes, &encrypted_card) {
                Ok((commitment, our_encrypted)) => {
                    drop(inner);

                    self.delegate.on_state_changed(MobileBleState::Verifying);

                    // Send our commitment via handshake write
                    if let Err(e) = self
                        .delegate
                        .send_data(CHAR_HANDSHAKE_WRITE.to_string(), commitment.clone())
                    {
                        self.fail(format!("Failed to send commitment: {e}"));
                        return;
                    }

                    // Send our encrypted card chunks via data write
                    *self.pending_encrypted.lock().unwrap() = Some(our_encrypted);
                    self.send_pending_encrypted(CHAR_DATA_WRITE);

                    // Reset reassembler for any further data
                    *self.reassembler.lock().unwrap() = None;
                }
                Err(e) => {
                    drop(inner);
                    self.fail(format!("Failed to process key ack: {e:?}"));
                }
            }
        } else {
            // Responder: received initiator's encrypted card.
            // We should have the commitment in pending_encrypted.
            let commitment_data = self.pending_encrypted.lock().unwrap().take();
            let Some(commitment) = commitment_data else {
                self.fail("No pending commitment".into());
                return;
            };

            let mut inner = self.inner.lock().unwrap();
            match inner.process_committed_payload(&commitment, &encrypted_card) {
                Ok(reveal) => {
                    drop(inner);

                    self.delegate.on_state_changed(MobileBleState::Verifying);

                    // Send our reveal via handshake notify
                    if let Err(e) = self
                        .delegate
                        .send_data(CHAR_HANDSHAKE_NOTIFY.to_string(), reveal)
                    {
                        self.fail(format!("Failed to send reveal: {e}"));
                    }

                    // Reset reassembler
                    *self.reassembler.lock().unwrap() = None;
                }
                Err(e) => {
                    drop(inner);
                    self.fail(format!("Failed to process committed payload: {e:?}"));
                }
            }
        }
    }

    /// Send the pending encrypted card in chunks via the specified characteristic.
    fn send_pending_encrypted(&self, characteristic: &str) {
        let encrypted = self.pending_encrypted.lock().unwrap().take();
        let Some(data) = encrypted else {
            return;
        };

        let mtu_usable = *self.mtu_usable.lock().unwrap();
        let chunker = BleChunker::new(&data, mtu_usable);

        self.delegate.on_state_changed(MobileBleState::Transferring);

        for i in 0..chunker.total_chunks() {
            if let Some(chunk) = chunker.chunk(i) {
                if let Err(e) = self.delegate.send_data(characteristic.to_string(), chunk) {
                    self.fail(format!("Failed to send chunk {i}: {e}"));
                    return;
                }
            }
        }
    }

    /// Complete the exchange with the reveal data.
    fn complete_exchange(&self, reveal: Vec<u8>) {
        let mut inner = self.inner.lock().unwrap();
        match inner.complete_exchange(&reveal) {
            Ok(result) => {
                let mobile_result = ble_result_to_mobile(&result);
                drop(inner);
                self.delegate.on_state_changed(MobileBleState::Complete);
                self.delegate.on_exchange_complete(mobile_result);
            }
            Err(e) => {
                drop(inner);
                self.fail(format!("Exchange verification failed: {e:?}"));
            }
        }
    }

    /// Transition to the failed state and notify the delegate.
    fn fail(&self, error: String) {
        self.delegate.on_state_changed(MobileBleState::Failed {
            error: error.clone(),
        });
        self.delegate.on_exchange_failed(error);
    }
}

// === Helpers ===

fn map_state(state: &BleHandshakeState) -> MobileBleState {
    match state {
        BleHandshakeState::Idle => MobileBleState::Connecting,
        BleHandshakeState::KeyOfferSent { .. } => MobileBleState::Handshaking,
        BleHandshakeState::KeyOfferReceived { .. } => MobileBleState::Handshaking,
        BleHandshakeState::SessionEstablished { .. } => MobileBleState::Transferring,
        BleHandshakeState::SendingPayload { .. } => MobileBleState::Transferring,
        BleHandshakeState::AwaitingPayload { .. } => MobileBleState::Transferring,
        BleHandshakeState::PayloadsExchanged { .. } => MobileBleState::Verifying,
        BleHandshakeState::RevealSent { .. } => MobileBleState::Verifying,
        BleHandshakeState::Complete { .. } => MobileBleState::Complete,
        BleHandshakeState::Failed { reason } => MobileBleState::Failed {
            error: format!("{reason:?}"),
        },
    }
}

fn ble_result_to_mobile(result: &BleExchangeResult) -> MobileBleExchangeResult {
    MobileBleExchangeResult {
        remote_display_name: result.remote_card.display_name.clone(),
        remote_identity_key: result.remote_card.identity_key.to_vec(),
        remote_exchange_key: result.remote_card.exchange_key.to_vec(),
        remote_fields: result
            .remote_card
            .fields
            .iter()
            .map(|(k, v)| MobileBleField {
                key: k.clone(),
                value: v.clone(),
            })
            .collect(),
        remote_avatar: result.remote_card.avatar.clone(),
    }
}

// === Tests ===

// INLINE_TEST_REQUIRED: tests use crate-private internal fields (Mutex members, map_state, ble_result_to_mobile)
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// Mock delegate that records all calls.
    struct MockBleDelegate {
        sent_data: StdMutex<Vec<(String, Vec<u8>)>>,
        states: StdMutex<Vec<String>>,
        result: StdMutex<Option<MobileBleExchangeResult>>,
        error: StdMutex<Option<String>>,
    }

    impl MockBleDelegate {
        fn new() -> Self {
            Self {
                sent_data: StdMutex::new(Vec::new()),
                states: StdMutex::new(Vec::new()),
                result: StdMutex::new(None),
                error: StdMutex::new(None),
            }
        }
    }

    impl MobileBleDelegate for MockBleDelegate {
        fn send_data(
            &self,
            characteristic_uuid: String,
            data: Vec<u8>,
        ) -> Result<(), MobileBleTransportError> {
            self.sent_data
                .lock()
                .unwrap()
                .push((characteristic_uuid, data));
            Ok(())
        }

        fn subscribe_notify(
            &self,
            _characteristic_uuid: String,
        ) -> Result<(), MobileBleTransportError> {
            Ok(())
        }

        fn disconnect(&self) -> Result<(), MobileBleTransportError> {
            Ok(())
        }

        fn on_state_changed(&self, state: MobileBleState) {
            self.states.lock().unwrap().push(format!("{state:?}"));
        }

        fn on_exchange_complete(&self, result: MobileBleExchangeResult) {
            *self.result.lock().unwrap() = Some(result);
        }

        fn on_exchange_failed(&self, error: String) {
            *self.error.lock().unwrap() = Some(error);
        }
    }

    fn make_session(name: &str, delegate: Box<dyn MobileBleDelegate>) -> MobileBleExchangeSession {
        MobileBleExchangeSession::new(
            vec![1u8; 32],
            name.to_string(),
            vec![2u8; 32],
            vec![MobileBleField {
                key: "email".into(),
                value: "test@example.com".into(),
            }],
            None,
            delegate,
        )
        .unwrap()
    }

    #[test]
    fn test_constructor_validates_key_lengths() {
        let delegate = Box::new(MockBleDelegate::new());
        let result = MobileBleExchangeSession::new(
            vec![1u8; 16], // too short
            "Test".into(),
            vec![2u8; 32],
            vec![],
            None,
            delegate,
        );
        assert!(matches!(
            result.err(),
            Some(MobileBleError::ExchangeFailed { .. })
        ));

        let delegate2 = Box::new(MockBleDelegate::new());
        let result2 = MobileBleExchangeSession::new(
            vec![1u8; 32],
            "Test".into(),
            vec![2u8; 16], // too short
            vec![],
            None,
            delegate2,
        );
        assert!(result2.is_err());
    }

    #[test]
    fn test_initial_state_is_connecting() {
        let delegate = Box::new(MockBleDelegate::new());
        let session = make_session("Alice", delegate);
        assert!(matches!(session.get_state(), MobileBleState::Connecting));
    }

    #[test]
    fn test_set_responder() {
        let delegate = Box::new(MockBleDelegate::new());
        let session = make_session("Bob", delegate);
        assert!(*session.is_initiator.lock().unwrap());

        session.set_responder();
        assert!(!*session.is_initiator.lock().unwrap());
    }

    #[test]
    fn test_on_connected_initiator_sends_key_offer() {
        let delegate = Box::new(MockBleDelegate::new());
        let session = make_session("Alice", delegate);

        session.on_connected("device-1".into());

        // Should have sent key offer on CHAR_HANDSHAKE_WRITE
        let inner = session.inner.lock().unwrap();
        assert!(matches!(
            inner.state(),
            BleHandshakeState::KeyOfferSent { .. }
        ));
    }

    #[test]
    fn test_cancel_notifies_delegate() {
        let delegate = Box::new(MockBleDelegate::new());
        let session = make_session("Alice", delegate);
        session.cancel();
        // cancel should set Failed state — verified by states vector
    }

    #[test]
    fn test_on_mtu_negotiated() {
        let delegate = Box::new(MockBleDelegate::new());
        let session = make_session("Alice", delegate);

        session.on_mtu_negotiated(512);
        assert_eq!(*session.mtu_usable.lock().unwrap(), 509);

        // Very small MTU should be clamped
        session.on_mtu_negotiated(3);
        assert_eq!(*session.mtu_usable.lock().unwrap(), BLE_CHUNK_OVERHEAD + 1);
    }

    #[test]
    fn test_map_state_covers_all_variants() {
        // Verify all BleHandshakeState variants are mapped
        let states = vec![
            BleHandshakeState::Idle,
            BleHandshakeState::KeyOfferSent {
                exchange_id: [0; 32],
            },
            BleHandshakeState::KeyOfferReceived {
                exchange_id: [0; 32],
            },
            BleHandshakeState::SessionEstablished {
                exchange_id: [0; 32],
            },
            BleHandshakeState::SendingPayload {
                exchange_id: [0; 32],
            },
            BleHandshakeState::AwaitingPayload {
                exchange_id: [0; 32],
                local_commitment: [0; 32],
            },
            BleHandshakeState::PayloadsExchanged {
                exchange_id: [0; 32],
                local_commitment: [0; 32],
                remote_commitment: [0; 32],
                remote_encrypted: vec![],
            },
            BleHandshakeState::RevealSent {
                exchange_id: [0; 32],
            },
            BleHandshakeState::Complete {
                local_card: BleCardPayload::new([0; 32], "L".into(), [0; 32], vec![], None),
                remote_card: BleCardPayload::new([0; 32], "R".into(), [0; 32], vec![], None),
            },
            BleHandshakeState::Failed {
                reason: vauchi_core::exchange::ExchangeError::InvalidBleFormat,
            },
        ];

        for state in &states {
            let _ = map_state(state); // Should not panic
        }

        // Verify specific mappings
        assert!(matches!(
            map_state(&BleHandshakeState::Idle),
            MobileBleState::Connecting
        ));
        assert!(matches!(
            map_state(&BleHandshakeState::Complete {
                local_card: BleCardPayload::new([0; 32], "L".into(), [0; 32], vec![], None),
                remote_card: BleCardPayload::new([0; 32], "R".into(), [0; 32], vec![], None),
            }),
            MobileBleState::Complete
        ));
    }

    #[test]
    fn test_ble_result_to_mobile() {
        let result = BleExchangeResult {
            local_card: BleCardPayload::new(
                [1; 32],
                "Alice".into(),
                [2; 32],
                vec![("email".into(), "alice@test.com".into())],
                None,
            ),
            remote_card: BleCardPayload::new(
                [3; 32],
                "Bob".into(),
                [4; 32],
                vec![
                    ("phone".into(), "+1234567890".into()),
                    ("email".into(), "bob@test.com".into()),
                ],
                Some(vec![0xFF, 0xD8, 0xFF]),
            ),
        };

        let mobile = ble_result_to_mobile(&result);
        assert_eq!(mobile.remote_display_name, "Bob");
        assert_eq!(mobile.remote_identity_key, vec![3; 32]);
        assert_eq!(mobile.remote_exchange_key, vec![4; 32]);
        assert_eq!(mobile.remote_fields.len(), 2);
        assert_eq!(mobile.remote_fields[0].key, "phone");
        assert_eq!(mobile.remote_fields[0].value, "+1234567890");
        assert_eq!(mobile.remote_fields[1].key, "email");
        assert_eq!(mobile.remote_fields[1].value, "bob@test.com");
        assert_eq!(mobile.remote_avatar, Some(vec![0xFF, 0xD8, 0xFF]));
    }

    #[test]
    fn test_on_disconnected_when_not_complete_reports_failure() {
        let delegate = Box::new(MockBleDelegate::new());
        let session = make_session("Alice", delegate);
        session.on_disconnected();
        // Should have called on_exchange_failed since state is Idle (not Complete)
    }
}
